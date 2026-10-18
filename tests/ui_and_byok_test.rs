#[cfg(test)]
mod ui_and_byok_tests {
    use rust_decimal::Decimal;
    use tally_mcp::byok::{is_host_allowed, mock_or_parse_invoice, Document};
    use tally_mcp::config::Config;
    use tally_mcp::journal::JournalManager;
    use tally_mcp::models::{compute_content_hash, DraftStatus, VoucherDraft, VoucherEntry};
    use tally_mcp::ui::CompanionUiServer;

    #[test]
    fn test_egress_allow_list() {
        let extra_hosts = vec!["custom-model.internal".to_string()];

        // Built-in hosts allowed
        assert!(is_host_allowed("api.anthropic.com", &extra_hosts));
        assert!(is_host_allowed(
            "generativelanguage.googleapis.com",
            &extra_hosts
        ));
        assert!(is_host_allowed("127.0.0.1", &extra_hosts));
        assert!(is_host_allowed("custom-model.internal", &extra_hosts));

        // Malicious or unlisted host strictly blocked
        assert!(!is_host_allowed("evil-exfiltration.com", &extra_hosts));
        assert!(!is_host_allowed("attacker.site", &extra_hosts));
    }

    #[test]
    fn test_document_extraction() {
        let invoice_json = br#"{
            "doc_type": "purchase_invoice",
            "party_name": "Standard Supplies Ltd",
            "party_gstin": "27AAAAA0000A1Z5",
            "invoice_no": "INV-1042",
            "date": "2026-10-12",
            "currency": "INR",
            "lines": [{
                "description": "Component",
                "hsn": "8544",
                "qty": 10,
                "rate": 45,
                "amount": 450,
                "tax_rate": 18
            }],
            "taxes": [{
                "type": "CGST",
                "rate": 9,
                "amount": 40.5
            }],
            "round_off": 0,
            "total": 531,
            "confidence": { "total": 0.99 },
            "source": { "file": "invoice_1042.json", "sha256": "" }
        }"#;

        let doc = Document {
            mime: "application/json".to_string(),
            bytes: invoice_json.to_vec(),
            name: "invoice_1042.json".to_string(),
        };

        let extraction = mock_or_parse_invoice(&doc).expect("Extraction should succeed");
        assert_eq!(extraction.doc_type, "purchase_invoice");
        assert_eq!(extraction.party_name, "Standard Supplies Ltd");
        assert_eq!(extraction.total, Decimal::from(531));
        assert_eq!(extraction.source.file, "invoice_1042.json");
        assert!(!extraction.source.sha256.is_empty());
    }

    #[test]
    fn test_companion_ui_token_and_approval_flow() {
        let config = Config::default();
        let ui_server = CompanionUiServer::new(config);
        assert!(!ui_server.launch_token().is_empty());

        let journal = JournalManager::new().expect("Failed to open journal");
        let draft_id = format!("d_ui_test_{}", uuid::Uuid::new_v4().simple());

        let mut draft = VoucherDraft {
            draft_id: draft_id.clone(),
            company: "Acme Corp".to_string(),
            voucher_type: "Purchase".to_string(),
            date: "20261012".to_string(),
            number: "INV-1042".to_string(),
            party_ledger: "Acme Corp".to_string(),
            entries: vec![
                VoucherEntry {
                    ledger: "Purchases".to_string(),
                    amount: Decimal::from(-100),
                },
                VoucherEntry {
                    ledger: "Acme Corp".to_string(),
                    amount: Decimal::from(100),
                },
            ],
            narration: "UI test".to_string(),
            idempotency_key: "idemp_test".to_string(),
            content_hash: String::new(),
            status: DraftStatus::PendingApproval,
            tally_guid: Some("guid-test".to_string()),
        };
        draft.content_hash = compute_content_hash(&draft);
        journal.save_draft(&draft).expect("Failed to save draft");

        // Verify draft loaded
        let loaded = journal.get_draft(&draft_id).expect("Draft should be saved");
        assert_eq!(loaded.status, DraftStatus::PendingApproval);
    }

    #[test]
    fn test_byok_key_storage_and_masking() {
        use tally_mcp::byok::{get_api_key, get_masked_api_key, set_api_key};

        // Ollama needs no key
        assert_eq!(
            get_masked_api_key("ollama"),
            Some("Local (No key required)".to_string())
        );

        // Anthropic key setting and retrieval
        let test_key = "sk-ant-api03-testkey1234567890abcdef";
        set_api_key("anthropic", test_key).expect("Key should save");
        assert_eq!(get_api_key("anthropic"), Some(test_key.to_string()));

        let masked = get_masked_api_key("anthropic").expect("Masked key should exist");
        assert!(masked.contains("••••"));
        assert!(masked.starts_with("sk-a"));
        assert!(masked.ends_with("cdef"));
    }

    #[test]
    fn test_config_persistence() {
        use tally_mcp::config::{save_config, Config};

        let mut config = Config::default();
        config.tally.host = "192.168.1.188".to_string();
        config.tally.port = 9005;
        config.tally.allow_remote_tally = true;
        config.ai.provider = "ollama".to_string();

        let path = save_config(&config).expect("save_config should succeed");
        assert!(path.exists());
    }

    #[test]
    fn test_gemini_missing_api_key_error_handling() {
        use tally_mcp::byok::{extract_document, Document, ExtractError};

        let mut config = Config::default();
        config.ai.provider = "gemini".to_string();
        config.ai.model = String::new(); // should default to gemini-3.8-flash

        let doc = Document {
            mime: "image/png".to_string(),
            bytes: vec![0x89, 0x50, 0x4E, 0x47],
            name: "invoice.png".to_string(),
        };

        // When Gemini API key is missing or not configured, it should return MissingApiKey cleanly
        let res = extract_document(&config, &doc);
        match res {
            Err(ExtractError::MissingApiKey(p)) => {
                assert_eq!(p, "gemini");
            }
            Err(e) => {
                let err_str = e.to_string();
                assert!(
                    err_str.contains("gemini") || err_str.contains("API Key"),
                    "Expected clean error regarding API key for gemini, got: {}",
                    err_str
                );
            }
            Ok(_) => panic!("Extraction should not succeed with missing key"),
        }
    }

    #[tokio::test]
    async fn test_mcp_server_jsonrpc_handling() {
        use serde_json::json;
        use tally_mcp::mcp::McpServer;

        let config = Config::default();
        let mcp = McpServer::new(config);

        // Test initialize
        let init_resp = mcp
            .handle_request(
                "initialize",
                json!({ "protocolVersion": "2024-11-05" }),
                json!(1),
            )
            .await;
        assert_eq!(init_resp["id"], 1);
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "tally-mcp");

        // Test tools/list
        let tools_resp = mcp.handle_request("tools/list", json!({}), json!(2)).await;
        assert_eq!(tools_resp["id"], 2);
        let tools = tools_resp["result"]["tools"]
            .as_array()
            .expect("Tools must be an array");
        assert!(!tools.is_empty(), "Tools list should not be empty");
    }

    #[test]
    fn test_guide_html_endpoints_and_links() {
        let index_html = include_str!("../src/ui/assets/index.html");
        assert!(
            index_html.contains("guides.html"),
            "index.html must link to guides.html"
        );
        assert!(
            index_html.contains("guide-claude.html"),
            "index.html must link to guide-claude.html"
        );
        assert!(
            index_html.contains("guide-chatgpt.html"),
            "index.html must link to guide-chatgpt.html"
        );
        assert!(
            index_html.contains("guide-gemini.html"),
            "index.html must link to guide-gemini.html"
        );
        assert!(
            index_html.contains("guide-cursor.html"),
            "index.html must link to guide-cursor.html"
        );
        assert!(
            index_html.contains("guide-windsurf.html"),
            "index.html must link to guide-windsurf.html"
        );
        assert!(
            index_html.contains("guide-cline.html"),
            "index.html must link to guide-cline.html"
        );
        assert!(
            index_html.contains("ipconfig"),
            "index.html must provide ipconfig instructions for discovering LAN IP"
        );
        assert!(
            index_html.contains("toggleLanIpGuide"),
            "index.html must provide toggleLanIpGuide function"
        );
        assert!(
            index_html.contains("tallymcp.pro"),
            "index.html must use tallymcp.pro as MCP server identifier"
        );
        assert!(
            index_html.contains("draft-modal"),
            "index.html must contain draft details review modal"
        );

        let claude_guide = include_str!("../src/ui/assets/guide-claude.html");
        assert!(
            claude_guide.contains("Claude Desktop"),
            "Claude guide should mention Claude Desktop"
        );
        assert!(
            claude_guide.contains("ipconfig"),
            "Claude guide should mention ipconfig for finding LAN IP"
        );
        assert!(
            claude_guide.contains("tallymcp.pro"),
            "Claude guide must mention tallymcp.pro server identifier"
        );
        assert!(
            claude_guide.contains("Connectors"),
            "Claude guide must guide user to check Connectors in Claude"
        );
        assert!(
            claude_guide.contains("Add custom connector"),
            "Claude guide should detail custom connectors"
        );
        assert!(
            claude_guide.contains("Step 1 of 2"),
            "Claude guide should mention Step 1 of 2"
        );

        let chatgpt_guide = include_str!("../src/ui/assets/guide-chatgpt.html");
        assert!(
            chatgpt_guide.contains("ChatGPT Desktop"),
            "ChatGPT guide must mention ChatGPT Desktop"
        );

        let gemini_guide = include_str!("../src/ui/assets/guide-gemini.html");
        assert!(
            gemini_guide.contains("gemini-3.8-flash"),
            "Gemini guide must mention default model gemini-3.8-flash"
        );

        let cursor_guide = include_str!("../src/ui/assets/guide-cursor.html");
        assert!(
            cursor_guide.contains(".cursor/mcp.json"),
            "Cursor guide must detail .cursor/mcp.json"
        );

        let windsurf_guide = include_str!("../src/ui/assets/guide-windsurf.html");
        assert!(
            windsurf_guide.contains("mcp_config.json"),
            "Windsurf guide must detail mcp_config.json"
        );

        let cline_guide = include_str!("../src/ui/assets/guide-cline.html");
        assert!(
            cline_guide.contains("cline_mcp_settings.json"),
            "Cline guide must detail cline_mcp_settings.json"
        );

        let hub_guide = include_str!("../src/ui/assets/guides.html");
        assert!(
            hub_guide.contains("TallyMCP Connectors Guide Hub"),
            "Hub guide must contain header"
        );
    }

    #[test]
    fn test_chat_ui_and_stepper_elements() {
        let index_html = include_str!("../src/ui/assets/index.html");
        assert!(
            index_html.contains("setup-stepper"),
            "Must contain progressive setup stepper"
        );
        assert!(
            index_html.contains("chat-container"),
            "Must contain chat container"
        );
        assert!(
            index_html.contains("chat-templates"),
            "Must contain clickable FAQ templates"
        );
        assert!(
            index_html.contains("Show Trial Balance summary"),
            "Must contain Trial Balance template"
        );
        assert!(
            index_html.contains("chat-byok-warning"),
            "Must check for BYOK configuration before enabling"
        );
        assert!(
            index_html.contains("claude_desktop_config.json"),
            "Must guide on Claude desktop config"
        );
        assert!(
            index_html.contains("tabbtn-tools"),
            "Must contain MCP Functions & Testing tab button"
        );
        assert!(
            index_html.contains("tab-tools"),
            "Must contain tab-tools section"
        );
        assert!(
            index_html.contains("tally_list_companies"),
            "Must provide testing for tally_list_companies"
        );
        assert!(
            index_html.contains("runPrePushDiagnosticSuite"),
            "Must provide pre-push diagnostic suite runner"
        );
    }

    #[tokio::test]
    async fn test_mcp_functions_metadata_and_testing() {
        use serde_json::json;
        use tally_mcp::config::Config;
        use tally_mcp::mcp::McpServer;

        let config = Config::default();
        let mcp = McpServer::new(config);

        // 1. Check list_tools_metadata
        let tools_meta = mcp.list_tools_metadata();
        assert!(!tools_meta.is_empty(), "Tools metadata should not be empty");

        // Verify tally_list_companies is registered as read-only / GET method
        let list_comp = tools_meta
            .iter()
            .find(|t| t["name"] == "tally_list_companies")
            .expect("tally_list_companies must be present in tools metadata");
        assert_eq!(list_comp["access"], "read");
        assert_eq!(list_comp["is_get"], true);

        // Verify tally_status is registered as read-only / GET method
        let status_tool = tools_meta
            .iter()
            .find(|t| t["name"] == "tally_status")
            .expect("tally_status must be present in tools metadata");
        assert_eq!(status_tool["access"], "read");
        assert_eq!(status_tool["is_get"], true);

        // 2. Test calling tally_status tool
        let status_resp = mcp.call_tool("tally_status", json!({}), json!(101)).await;
        assert_eq!(status_resp["id"], 101);
        let content_arr = status_resp["result"]["content"]
            .as_array()
            .expect("Status response should contain content array");
        let status_text = content_arr[0]["text"]
            .as_str()
            .expect("Should have text in content");
        assert!(
            status_text.contains("reachable"),
            "Status text must contain reachability status: {}",
            status_text
        );

        // 3. Test calling tally_list_companies tool
        let comp_resp = mcp
            .call_tool("tally_list_companies", json!({}), json!(102))
            .await;
        assert_eq!(comp_resp["id"], 102);
        assert!(
            comp_resp.get("result").is_some(),
            "list_companies tool must return a result object"
        );
    }

    #[test]
    fn test_stored_zip_generation_for_mcpb() {
        use tally_mcp::ui::create_stored_zip;

        let manifest = br#"{"dxt_version":"0.1","name":"tally-mcp"}"#;
        let zip = create_stored_zip("manifest.json", manifest);

        // Check local file header signature PK\x03\x04
        assert_eq!(&zip[0..4], b"PK\x03\x04");

        // Check file name in local header
        assert_eq!(&zip[30..43], b"manifest.json");

        // Check manifest content is included
        let content_start = 43;
        let content_end = content_start + manifest.len();
        assert_eq!(&zip[content_start..content_end], manifest);

        // Check Central Directory header PK\x01\x02
        let cd_start = content_end;
        assert_eq!(&zip[cd_start..cd_start + 4], b"PK\x01\x02");

        // Check End of Central Directory record signature PK\x05\x06 at the end
        let eocd_len = 22;
        let eocd_start = zip.len() - eocd_len;
        assert_eq!(&zip[eocd_start..eocd_start + 4], b"PK\x05\x06");
    }

    #[test]
    fn test_client_auto_configure_and_diagnose() {
        use tally_mcp::config::{
            diagnose_client_config, get_client_config_path, resolve_executable_path,
        };

        let claude_path = get_client_config_path("claude");
        assert!(
            claude_path.is_some(),
            "Claude path must resolve on current OS"
        );

        let diag = diagnose_client_config("claude");
        assert_eq!(diag.client, "claude");

        let exe = resolve_executable_path();
        assert!(
            !exe.is_empty(),
            "Resolved executable path must not be empty"
        );
    }

    #[test]
    fn test_companion_ui_mcp_api_endpoints() {
        std::env::set_var("TALLY_NO_OPEN", "1");
        let config = Config::default();
        let ui_server = CompanionUiServer::new(config);

        let port = 19876;
        std::thread::spawn(move || {
            let _ = ui_server.run(port);
        });

        std::thread::sleep(std::time::Duration::from_millis(200));

        let url_tools = format!("http://127.0.0.1:{}/api/mcp/tools", port);
        if let Ok(resp) = ureq::get(&url_tools).call() {
            let body: serde_json::Value = resp.into_body().read_json().expect("Valid JSON");
            assert_eq!(body["success"], true);
            let tools = body["tools"].as_array().expect("tools array");
            assert!(tools.iter().any(|t| t["name"] == "tally_list_companies"));
            assert!(tools.iter().any(|t| t["name"] == "tally_status"));
        }

        // Test POST /api/mcp/test-tool for tally_status
        let url_test = format!("http://127.0.0.1:{}/api/mcp/test-tool", port);
        if let Ok(resp) = ureq::post(&url_test)
            .header("content-type", "application/json")
            .send_json(serde_json::json!({
                "tool": "tally_status",
                "arguments": {}
            }))
        {
            let body: serde_json::Value = resp.into_body().read_json().expect("Valid JSON");
            assert_eq!(body["tool"], "tally_status");
            assert!(body.get("latency_ms").is_some());
        }

        // Test GET /approve route returns UI HTML
        let url_approve = format!("http://127.0.0.1:{}/approve?draft_id=d_123&hash=abc", port);
        if let Ok(resp) = ureq::get(&url_approve).call() {
            let body_str = resp.into_body().read_to_string().expect("HTML body");
            assert!(body_str.contains("TallyPrime"));
        }

        // Test POST /api/safety/config to switch write_mode
        let url_safety = format!("http://127.0.0.1:{}/api/safety/config", port);
        if let Ok(resp) = ureq::post(&url_safety)
            .header("content-type", "application/json")
            .send_json(serde_json::json!({
                "write_mode": "approve"
            }))
        {
            let body: serde_json::Value = resp.into_body().read_json().expect("Valid JSON");
            assert_eq!(body["success"], true);
            assert_eq!(body["write_mode"], "approve");
        }
    }
}
