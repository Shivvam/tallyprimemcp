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

        let claude_guide = include_str!("../src/ui/assets/guide-claude.html");
        assert!(
            claude_guide.contains("Claude Desktop"),
            "Claude guide should mention Claude Desktop"
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
}
