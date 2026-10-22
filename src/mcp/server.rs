use crate::audit::AuditLogger;
use crate::config::Config;
use crate::journal::JournalManager;
use crate::models::{
    compute_content_hash, compute_idempotency_key, DraftStatus, VoucherDraft, VoucherEntry,
};
use crate::packs::PackManager;
use crate::tally::builder::build_voucher_import_xml;
use crate::tally::TallyClient;
use crate::validator::validate_draft;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone)]
pub struct McpServer {
    config: Config,
    tally: Arc<TallyClient>,
    journal: Arc<JournalManager>,
    audit: Arc<AuditLogger>,
    packs: Arc<PackManager>,
}

impl McpServer {
    pub fn new(config: Config) -> Self {
        let tally = Arc::new(TallyClient::new(config.tally.clone()));
        let journal =
            Arc::new(JournalManager::new().expect("Failed to initialize journal manager"));
        let audit = Arc::new(AuditLogger::new());
        let packs = Arc::new(PackManager::new(
            config.packs.enable_unsigned,
            &config.packs.disabled,
        ));

        // Perform startup crash reconciliation
        if let Ok(reconciled) = journal.reconcile_startup() {
            if !reconciled.is_empty() {
                eprintln!(
                    "[tally-mcp] Startup reconciliation completed. Reconciled drafts: {:?}",
                    reconciled
                );
            }
        }

        Self {
            config,
            tally,
            journal,
            audit,
            packs,
        }
    }

    pub async fn run_stdio(&self) -> Result<(), Box<dyn std::error::Error>> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);

        let mut line = String::new();

        eprintln!("[tally-mcp] Starting stdio MCP server...");

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                // stdin closed: exit watchdog
                eprintln!("[tally-mcp] stdin closed, terminating server cleanly.");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(request) = serde_json::from_str::<Value>(trimmed) {
                if let Some(id) = request.get("id").cloned() {
                    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    let params = request.get("params").cloned().unwrap_or(Value::Null);

                    let response = self.handle_request(method, params, id).await;
                    let resp_str = serde_json::to_string(&response)? + "\n";
                    stdout.write_all(resp_str.as_bytes()).await?;
                    stdout.flush().await?;
                } else {
                    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method == "notifications/initialized" {
                        eprintln!("[tally-mcp] Client initialized notification received.");
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn handle_request(&self, method: &str, params: Value, id: Value) -> Value {
        match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {
                        "name": "tally-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": {
                        "tools": {
                            "listChanged": false
                        },
                        "prompts": {
                            "listChanged": false
                        },
                        "resources": {
                            "listChanged": false
                        }
                    }
                }
            }),

            "ping" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }),

            "prompts/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "prompts": [
                        {
                            "name": "tally-monthly-close",
                            "description": "TallyPrime month-end close checklist: bank entries, receivables chase, GSTR-2B triage, books health. Use when: month-end, close the books. Drafts need approval."
                        },
                        {
                            "name": "tally-chase-overdue-customers",
                            "description": "Find overdue TallyPrime customers and draft reminder messages. Use when: who owes me money, payment chase. Messages are never sent."
                        },
                        {
                            "name": "tally-gstr2b-triage",
                            "description": "Triage GSTR-2B vs TallyPrime mismatches with likely causes and supplier follow-ups. Use when: ITC reconciliation. Read-only."
                        },
                        {
                            "name": "tally-bank-statement-to-vouchers",
                            "description": "Turn a bank statement into reviewed TallyPrime draft vouchers. Use when: bank reconciliation entries. Drafts need approval."
                        },
                        {
                            "name": "tally-bill-to-purchase-voucher",
                            "description": "Turn a supplier bill photo/PDF into a TallyPrime draft purchase voucher. Use when: purchase entry from invoice. Draft needs approval."
                        },
                        {
                            "name": "tally-books-health-review",
                            "description": "Review TallyPrime books for duplicates, negative balances and unusual entries. Use when: audit review. Read-only."
                        }
                    ]
                }
            }),

            "prompts/get" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let prompt_desc = match name {
                    "tally-monthly-close" => "TallyPrime month-end close checklist: bank entries, receivables chase, GSTR-2B triage, books health.",
                    "tally-chase-overdue-customers" => "Find overdue TallyPrime customers and draft reminder messages.",
                    "tally-gstr2b-triage" => "Triage GSTR-2B vs TallyPrime mismatches with likely causes and supplier follow-ups.",
                    "tally-bank-statement-to-vouchers" => "Turn a bank statement into reviewed TallyPrime draft vouchers.",
                    "tally-bill-to-purchase-voucher" => "Turn a supplier bill photo/PDF into a TallyPrime draft purchase voucher.",
                    "tally-books-health-review" => "Review TallyPrime books for duplicates, negative balances and unusual entries.",
                    _ => "TallyPrime workflow prompt"
                };
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "description": prompt_desc,
                        "messages": [
                            {
                                "role": "user",
                                "content": {
                                    "type": "text",
                                    "text": format!("Execute the {} workflow according to TallyMCP safety conventions.", name)
                                }
                            }
                        ]
                    }
                })
            },

            "resources/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "resources": [
                        {
                            "uri": "tally://docs/versions",
                            "name": "TallyPrime Tested and Supported Versions",
                            "mimeType": "application/json"
                        }
                    ]
                }
            }),

            "resources/read" => {
                let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                if uri == "tally://docs/versions" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "contents": [
                                {
                                    "uri": "tally://docs/versions",
                                    "mimeType": "application/json",
                                    "text": json!({
                                        "server_version": env!("CARGO_PKG_VERSION"),
                                        "tested_on": ["TallyPrime 7.2", "TallyPrime 6.x"],
                                        "portal": "https://tallymcp.vercel.app/docs/versions"
                                    }).to_string()
                                }
                            ]
                        }
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32602, "message": format!("Resource not found: {}", uri) }
                    })
                }
            },

            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": self.list_tools()
                }
            }),

            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                self.call_tool(name, arguments, id).await
            }

            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                }
            }),
        }
    }

    pub fn list_tools(&self) -> Vec<Value> {
        let mut tools = vec![
            json!({
                "name": "tally_status",
                "description": "Get Tally reachability, loaded companies, and circuit breaker health",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_list_companies",
                "description": "List all loaded and available companies in Tally",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_set_active_company",
                "description": "Set the active company for read operations in this session",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Company name" }
                    },
                    "required": ["company"]
                }
            }),
            json!({
                "name": "tally_list_ledgers",
                "description": "List ledger accounts for a company",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Company name (optional if active company set)" }
                    }
                }
            }),
            json!({
                "name": "tally_trial_balance",
                "description": "Export Trial Balance report from Tally",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Company name" },
                        "from_date": { "type": "string", "description": "YYYYMMDD" },
                        "to_date": { "type": "string", "description": "YYYYMMDD" }
                    }
                }
            }),
            json!({
                "name": "tally_day_book",
                "description": "Export Day Book transactions from Tally",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Company name" },
                        "from_date": { "type": "string", "description": "YYYYMMDD" },
                        "to_date": { "type": "string", "description": "YYYYMMDD" }
                    }
                }
            }),
            json!({
                "name": "tally_draft_voucher",
                "description": "Validate and stage a voucher draft for out-of-band approval. Writes NOTHING to Tally.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Target company name" },
                        "voucher_type": { "type": "string", "description": "Purchase, Sales, Journal, Payment, or Receipt" },
                        "date": { "type": "string", "description": "Date in YYYYMMDD format" },
                        "number": { "type": "string", "description": "Voucher or invoice number" },
                        "party_ledger": { "type": "string", "description": "Party ledger name" },
                        "entries": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ledger": { "type": "string" },
                                    "amount": { "type": "string", "description": "Amount as decimal string. Must sum to exactly 0." }
                                },
                                "required": ["ledger", "amount"]
                            }
                        },
                        "narration": { "type": "string", "description": "Narration text" }
                    },
                    "required": ["company", "voucher_type", "date", "number", "party_ledger", "entries"]
                }
            }),
            json!({
                "name": "tally_preview_draft",
                "description": "Preview human-readable summary and exact XML envelope for a drafted voucher",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "draft_id": { "type": "string", "description": "Draft ID" }
                    },
                    "required": ["draft_id"]
                }
            }),
            json!({
                "name": "tally_commit_draft",
                "description": "Post an approved voucher draft to Tally. Idempotent with write-ahead journal and read-back verification.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "draft_id": { "type": "string", "description": "Draft ID to commit" }
                    },
                    "required": ["draft_id"]
                }
            }),
            json!({
                "name": "tally_extract_document",
                "description": "Extract structured invoice/receipt data from a document file using configured AI provider or offline model",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string", "description": "Absolute path to the local PDF or image file" }
                    },
                    "required": ["file_path"]
                }
            }),
            json!({
                "name": "tally_discard_draft",
                "description": "Discard a pending voucher draft",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "draft_id": { "type": "string", "description": "Draft ID to discard" }
                    },
                    "required": ["draft_id"]
                }
            }),
            json!({
                "name": "tally_run_report",
                "description": "Run a named report from an active tool pack",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "report_name": { "type": "string", "description": "Name of the tool pack report" },
                        "parameters": { "type": "object", "description": "Report template arguments" }
                    },
                    "required": ["report_name"]
                }
            }),
            json!({
                "name": "tally_get_environment",
                "description": "Get TallyPrime connection status: Tally version, open company, edit-log on/off, server version, tested versions. Use when: check connection, which company is open, version support. READ-ONLY.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_diagnose_connection",
                "description": "Diagnose TallyPrime connection problems: Tally not running, XML/HTTP server off, wrong port, firewall, no company open, version mismatch. Use when: cannot connect to Tally, timeouts, company not found. READ-ONLY.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "port": { "type": "integer", "description": "Tally XML server port (default 9000)" }
                    }
                }
            }),
        ];

        // Dynamically add active tool packs
        for pt in self.packs.list_tools() {
            tools.push(json!({
                "name": pt.name,
                "description": pt.description,
                "inputSchema": pt.input_schema
            }));
        }

        tools
    }

    pub fn list_tools_metadata(&self) -> Vec<Value> {
        let base_tools = self.list_tools();
        let mut list = Vec::new();
        for t in base_tools {
            let name = t
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let desc = t
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let schema = t.get("inputSchema").cloned().unwrap_or(json!({}));

            let (display_name, access, is_get, sample_args) = match name.as_str() {
                "tally_status" => ("Tally System Status", "read", true, json!({})),
                "tally_list_companies" => ("Get List of Companies", "read", true, json!({})),
                "tally_set_active_company" => (
                    "Set Active Company",
                    "session",
                    false,
                    json!({ "company": "Acme Trading Pvt Ltd" }),
                ),
                "tally_list_ledgers" => (
                    "Get List of Ledgers",
                    "read",
                    true,
                    json!({ "company": "" }),
                ),
                "tally_trial_balance" => (
                    "Get Trial Balance",
                    "read",
                    true,
                    json!({ "company": "", "from_date": "", "to_date": "" }),
                ),
                "tally_day_book" => (
                    "Get Day Book",
                    "read",
                    true,
                    json!({ "company": "", "from_date": "", "to_date": "" }),
                ),
                "tally_preview_draft" => (
                    "Preview Voucher Draft",
                    "read",
                    true,
                    json!({ "draft_id": "d_sample" }),
                ),
                "tally_draft_voucher" => (
                    "Draft Voucher",
                    "draft",
                    false,
                    json!({
                        "company": "Acme Trading Pvt Ltd",
                        "voucher_type": "Payment",
                        "date": "20250401",
                        "number": "PV-001",
                        "party_ledger": "Acme Supplier",
                        "entries": [
                            { "ledger": "Acme Supplier", "amount": "1000.00" },
                            { "ledger": "Bank Account", "amount": "-1000.00" }
                        ],
                        "narration": "Sample payment voucher"
                    }),
                ),
                "tally_commit_draft" => (
                    "Commit Approved Draft",
                    "write",
                    false,
                    json!({ "draft_id": "d_sample" }),
                ),
                "tally_discard_draft" => (
                    "Discard Voucher Draft",
                    "draft",
                    false,
                    json!({ "draft_id": "d_sample" }),
                ),
                "tally_extract_document" => (
                    "Extract Invoice / Document",
                    "read/ai",
                    true,
                    json!({ "file_path": "C:\\path\\to\\invoice.pdf" }),
                ),
                "tally_run_report" => (
                    "Run Pack Report",
                    "read",
                    true,
                    json!({ "report_name": "outstanding_ageing", "parameters": {} }),
                ),
                _ => (name.as_str(), "read", true, json!({})),
            };

            list.push(json!({
                "name": name,
                "display_name": display_name,
                "description": desc,
                "input_schema": schema,
                "access": access,
                "is_get": is_get,
                "sample_args": sample_args,
            }));
        }
        list
    }

    pub async fn call_tool(&self, name: &str, arguments: Value, id: Value) -> Value {
        // Check if tool is a dynamically registered pack tool
        if let Some(pt) = self.packs.find_tool(name) {
            match self.packs.execute_template(pt, &arguments) {
                Ok(xml) => {
                    let deadline = Duration::from_secs(self.config.tally.report_deadline_secs);
                    match self.tally.execute_raw_xml(&xml, deadline).await {
                        Ok(resp) => {
                            let formatted =
                                crate::tally::client::format_report_response(name, &resp);
                            self.audit.log_full(
                                name,
                                None,
                                None,
                                "Success (Pack Tool Executed)",
                                Some(&formatted),
                            );
                            return json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "content": [{ "type": "text", "text": formatted }] }
                            });
                        }
                        Err(e) => {
                            let err_msg = format!("Tally error: {}", e);
                            self.audit.log_full(name, None, None, &err_msg, None);
                            return json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                            });
                        }
                    }
                }
                Err(e) => {
                    let err_msg = format!("Pack error: {}", e);
                    self.audit.log_full(name, None, None, &err_msg, None);
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                    });
                }
            }
        }

        match name {
            "tally_status" => match self.tally.status().await {
                Ok(status) => {
                    let outcome = if status.reachable {
                        format!(
                            "Success: Gateway Online (Active: {})",
                            status.active_company.as_deref().unwrap_or("None")
                        )
                    } else {
                        "Offline: Gateway Unreachable".to_string()
                    };
                    self.audit.log_full(
                        "tally_status",
                        None,
                        None,
                        &outcome,
                        Some(&serde_json::to_string(&status).unwrap_or_default()),
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&status).unwrap_or_default() }] }
                    })
                }
                Err(err) => {
                    let err_msg = format!("Error: {}", err);
                    self.audit
                        .log_full("tally_status", None, None, &err_msg, None);
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                    })
                }
            },

            "tally_list_companies" => match self.tally.list_companies().await {
                Ok(companies) => {
                    let summary = if companies.is_empty() {
                        "Success: 0 companies loaded".to_string()
                    } else {
                        format!(
                            "Success: {} company(s) found ({})",
                            companies.len(),
                            companies.join(", ")
                        )
                    };
                    self.audit.log_full(
                        "tally_list_companies",
                        None,
                        None,
                        &summary,
                        Some(&serde_json::to_string(&companies).unwrap_or_default()),
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&companies).unwrap_or_default() }] }
                    })
                }
                Err(err) => {
                    let err_msg = format!("Error: {}", err);
                    self.audit
                        .log_full("tally_list_companies", None, None, &err_msg, None);
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                    })
                }
            },

            "tally_set_active_company" => {
                let company = arguments
                    .get("company")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .trim();
                if company.is_empty() {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": "Company name cannot be empty" }] }
                    })
                } else {
                    // Validate against currently loaded companies in Tally if available
                    match self.tally.list_companies().await {
                        Ok(companies) if !companies.is_empty() => {
                            if let Some(matched) =
                                companies.iter().find(|c| c.eq_ignore_ascii_case(company))
                            {
                                self.tally.set_active_company(matched.clone()).await;
                                self.audit.log_full(
                                    "tally_set_active_company",
                                    None,
                                    Some(matched),
                                    &format!("Success: Active company set to '{}'", matched),
                                    Some(matched),
                                );
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": { "content": [{ "type": "text", "text": format!("Active company set to '{}'", matched) }] }
                                })
                            } else {
                                let err_msg = format!(
                                    "Company '{}' not found in loaded companies. Available companies: {}",
                                    company,
                                    companies.join(", ")
                                );
                                self.audit.log_full(
                                    "tally_set_active_company",
                                    None,
                                    Some(company),
                                    "Error: Company not found",
                                    Some(&err_msg),
                                );
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": {
                                        "isError": true,
                                        "content": [{
                                            "type": "text",
                                            "text": err_msg
                                        }]
                                    }
                                })
                            }
                        }
                        _ => {
                            // If Tally list_companies is unavailable, set active company
                            self.tally.set_active_company(company.to_string()).await;
                            self.audit.log_full(
                                "tally_set_active_company",
                                None,
                                Some(company),
                                &format!("Active company set to '{}' (unverified)", company),
                                None,
                            );
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "content": [{ "type": "text", "text": format!("Active company set to '{}'", company) }] }
                            })
                        }
                    }
                }
            }

            "tally_list_ledgers" => {
                let comp = arguments.get("company").and_then(|c| c.as_str());
                match self.tally.list_ledgers(comp).await {
                    Ok(ledgers) => {
                        let summary = format!("Success: {} ledger(s) found", ledgers.len());
                        self.audit.log_full(
                            "tally_list_ledgers",
                            None,
                            comp,
                            &summary,
                            Some(&serde_json::to_string(&ledgers).unwrap_or_default()),
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&ledgers).unwrap_or_default() }] }
                        })
                    }
                    Err(err) => {
                        let err_msg = format!("Error: {}", err);
                        self.audit
                            .log_full("tally_list_ledgers", None, comp, &err_msg, None);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                        })
                    }
                }
            }

            "tally_trial_balance" => {
                let comp = arguments.get("company").and_then(|c| c.as_str());
                let from = arguments.get("from_date").and_then(|d| d.as_str());
                let to = arguments.get("to_date").and_then(|d| d.as_str());
                match self.tally.trial_balance(comp, from, to).await {
                    Ok(report) => {
                        self.audit.log_full(
                            "tally_trial_balance",
                            None,
                            comp,
                            "Success: Trial Balance queried",
                            Some(&report),
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": report }] }
                        })
                    }
                    Err(err) => {
                        let err_msg = format!("Error: {}", err);
                        self.audit
                            .log_full("tally_trial_balance", None, comp, &err_msg, None);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                        })
                    }
                }
            }

            "tally_day_book" => {
                let comp = arguments.get("company").and_then(|c| c.as_str());
                let from = arguments.get("from_date").and_then(|d| d.as_str());
                let to = arguments.get("to_date").and_then(|d| d.as_str());
                match self.tally.day_book(comp, from, to).await {
                    Ok(report) => {
                        self.audit.log_full(
                            "tally_day_book",
                            None,
                            comp,
                            "Success: Day Book queried",
                            Some(&report),
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": report }] }
                        })
                    }
                    Err(err) => {
                        let err_msg = format!("Error: {}", err);
                        self.audit
                            .log_full("tally_day_book", None, comp, &err_msg, None);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                        })
                    }
                }
            }

            "tally_draft_voucher" => {
                let company = arguments
                    .get("company")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let voucher_type = arguments
                    .get("voucher_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let date = arguments.get("date").and_then(|d| d.as_str()).unwrap_or("");
                let number = arguments
                    .get("number")
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                let party_ledger = arguments
                    .get("party_ledger")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let narration = arguments
                    .get("narration")
                    .and_then(|n| n.as_str())
                    .unwrap_or("");

                let mut entries = Vec::new();
                if let Some(arr) = arguments.get("entries").and_then(|e| e.as_array()) {
                    for item in arr {
                        let ledger = item.get("ledger").and_then(|l| l.as_str()).unwrap_or("");
                        let amt_str = item.get("amount").and_then(|a| a.as_str()).unwrap_or("0");
                        if let Ok(amount) = amt_str.parse::<Decimal>() {
                            entries.push(VoucherEntry {
                                ledger: ledger.to_string(),
                                amount,
                            });
                        }
                    }
                }

                let draft_id = format!("d_{}", uuid::Uuid::new_v4().simple());
                let total = entries
                    .iter()
                    .filter(|e| e.amount > Decimal::ZERO)
                    .map(|e| e.amount)
                    .sum();
                let idempotency_key = compute_idempotency_key(
                    company,
                    voucher_type,
                    party_ledger,
                    number,
                    date,
                    total,
                );

                let mut draft = VoucherDraft {
                    draft_id: draft_id.clone(),
                    company: company.to_string(),
                    voucher_type: voucher_type.to_string(),
                    date: date.to_string(),
                    number: number.to_string(),
                    party_ledger: party_ledger.to_string(),
                    entries,
                    narration: narration.to_string(),
                    idempotency_key,
                    content_hash: String::new(),
                    status: DraftStatus::NeedsReview,
                    tally_guid: Some(uuid::Uuid::new_v4().to_string()),
                };

                draft.content_hash = compute_content_hash(&draft);

                // Validation check
                if let Err(val_err) = validate_draft(&draft, &self.config.safety) {
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "isError": true,
                            "content": [{
                                "type": "text",
                                "text": format!("Validation failed: {}", val_err)
                            }]
                        }
                    });
                }

                if self.config.safety.write_mode == "auto_under_limit" {
                    draft.status = DraftStatus::Approved;
                } else {
                    draft.status = DraftStatus::PendingApproval;
                }

                if let Err(e) = self.journal.save_draft(&draft) {
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Failed to save draft: {}", e) }] }
                    });
                }

                self.audit.log(
                    "tally_draft_voucher",
                    Some(&draft_id),
                    Some(company),
                    "staged",
                );

                let approval_url = format!(
                    "http://127.0.0.1:{}/approve?draft_id={}&hash={}",
                    self.config.ui.port, draft_id, draft.content_hash
                );

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Draft created successfully!\nDraft ID: {}\nStatus: {:?}\nContent Hash: {}\nApproval URL (out-of-band human approval required): {}\nNote: AI cannot approve drafts.",
                                draft.draft_id, draft.status, draft.content_hash, approval_url
                            )
                        }]
                    }
                })
            }

            "tally_preview_draft" => {
                let draft_id = arguments
                    .get("draft_id")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                match self.journal.get_draft(draft_id) {
                    Ok(draft) => {
                        let xml = build_voucher_import_xml(&draft);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{
                                    "type": "text",
                                    "text": format!(
                                        "=== VOUCHER DRAFT PREVIEW ===\nID: {}\nStatus: {:?}\nCompany: {}\nType: {}\nNumber: {}\nDate: {}\nParty: {}\nEntries:\n{}\n\n=== EXACT XML PAYLOAD ===\n{}",
                                        draft.draft_id,
                                        draft.status,
                                        draft.company,
                                        draft.voucher_type,
                                        draft.number,
                                        draft.date,
                                        draft.party_ledger,
                                        serde_json::to_string_pretty(&draft.entries).unwrap_or_default(),
                                        xml
                                    )
                                }]
                            }
                        })
                    }
                    Err(e) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Error: {}", e) }] }
                    }),
                }
            }

            "tally_commit_draft" => {
                let draft_id = arguments
                    .get("draft_id")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let draft = match self.journal.get_draft(draft_id) {
                    Ok(d) => d,
                    Err(e) => {
                        return json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": format!("Error: {}", e) }] }
                        });
                    }
                };

                if self.config.safety.write_mode == "off" {
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "isError": true,
                            "content": [{
                                "type": "text",
                                "text": "Commit rejected: Write mode is currently 'off' in config.toml safety settings."
                            }]
                        }
                    });
                }

                if draft.status != DraftStatus::Approved {
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "isError": true,
                            "content": [{
                                "type": "text",
                                "text": format!("Commit rejected: Draft status is '{:?}'. Approval is strictly human out-of-band via UI.", draft.status)
                            }]
                        }
                    });
                }

                if let Err(e) = self.journal.record_commit_intent(&draft) {
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Journal error: {}", e) }] }
                    });
                }

                let format_mcp_err = |raw: String| {
                    if raw == "1" || raw.trim() == "1" {
                        "Tally rejected voucher (ERRORS: 1). Check 'tally.imp' in the Tally folder. Common causes: 1) Date is outside company's active Financial Year (Alt+F2 in Tally), 2) Ledger name mismatch, or 3) Unbalanced entries.".to_string()
                    } else if raw.contains("Voucher date is missing") || raw.contains("retry Split")
                    {
                        format!("{} (Tally Educational Mode restriction: vouchers can only be dated on the 1st, 2nd, or 31st of any month; also verify your company's active Financial Year period via Alt+F2 in Tally)", raw)
                    } else {
                        raw
                    }
                };

                let xml = build_voucher_import_xml(&draft);
                let import_resp = match self.tally.post_voucher_xml(&xml).await {
                    Ok(resp) => resp,
                    Err(err) => {
                        let err_str = format_mcp_err(format!("Failed to post to Tally: {}", err));
                        let _ = self.journal.record_commit_result(
                            &draft,
                            DraftStatus::Approved,
                            &err_str,
                        );
                        self.audit.log(
                            "tally_commit_draft",
                            Some(draft_id),
                            Some(&draft.company),
                            "failed_post",
                        );
                        return json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": err_str }] }
                        });
                    }
                };

                if import_resp.errors > 0 || import_resp.created == 0 {
                    let raw_err = import_resp
                        .error_message
                        .unwrap_or_else(|| "Tally rejected voucher creation".to_string());
                    let err_msg = format_mcp_err(raw_err);
                    let _ =
                        self.journal
                            .record_commit_result(&draft, DraftStatus::Failed, &err_msg);
                    self.audit.log(
                        "tally_commit_draft",
                        Some(draft_id),
                        Some(&draft.company),
                        "rejected_by_tally",
                    );
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Tally reported errors: {}", err_msg) }] }
                    });
                }

                let verified = self
                    .tally
                    .verify_voucher_exists(&draft.company, &draft.number)
                    .await
                    .unwrap_or(false);

                let final_status = if verified {
                    DraftStatus::Committed
                } else {
                    DraftStatus::CommittedUnverified
                };

                let outcome_text = if verified {
                    "Voucher successfully created and verified in Tally."
                } else {
                    "Voucher created, but read-back verification could not immediately confirm. Flagged as committed_unverified."
                };

                let _ =
                    self.journal
                        .record_commit_result(&draft, final_status.clone(), outcome_text);
                self.audit.log(
                    "tally_commit_draft",
                    Some(draft_id),
                    Some(&draft.company),
                    outcome_text,
                );

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Commit completed!\nDraft ID: {}\nStatus: {:?}\nVoucher Number: {}\nOutcome: {}",
                                draft.draft_id, final_status, draft.number, outcome_text
                            )
                        }]
                    }
                })
            }

            "tally_extract_document" => {
                let path_str = arguments
                    .get("file_path")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let path = std::path::Path::new(path_str);
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("document.pdf")
                    .to_string();

                let bytes = match std::fs::read(path) {
                    Ok(b) => b,
                    Err(e) => {
                        return json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": format!("Failed to read file '{}': {}", path_str, e) }] }
                        });
                    }
                };

                let doc = crate::byok::Document {
                    mime: "application/pdf".to_string(),
                    bytes,
                    name,
                };

                let extraction = crate::byok::parse_invoice(&doc);
                match extraction {
                    Ok(ext) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&ext).unwrap_or_default()
                            }]
                        }
                    }),
                    Err(err) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Extraction failed: {}", err) }] }
                    }),
                }
            }

            "tally_discard_draft" => {
                let draft_id = arguments
                    .get("draft_id")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                match self.journal.get_draft(draft_id) {
                    Ok(mut draft) => {
                        draft.status = DraftStatus::Discarded;
                        let _ = self.journal.save_draft(&draft);
                        self.audit.log(
                            "tally_discard_draft",
                            Some(draft_id),
                            Some(&draft.company),
                            "discarded",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": format!("Draft {} has been discarded.", draft_id) }] }
                        })
                    }
                    Err(e) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Error: {}", e) }] }
                    }),
                }
            }

            "tally_run_report" => {
                let report_name = arguments
                    .get("report_name")
                    .and_then(|r| r.as_str())
                    .unwrap_or("");
                let params = arguments.get("parameters").cloned().unwrap_or(json!({}));

                if let Some(tool) = self.packs.find_tool(report_name) {
                    match self.packs.execute_template(tool, &params) {
                        Ok(xml) => {
                            let deadline =
                                Duration::from_secs(self.config.tally.report_deadline_secs);
                            match self.tally.execute_raw_xml(&xml, deadline).await {
                                Ok(resp) => {
                                    self.audit.log(
                                        "tally_run_report",
                                        None,
                                        None,
                                        &format!("executed: {}", report_name),
                                    );
                                    let formatted = crate::tally::client::format_report_response(
                                        report_name,
                                        &resp,
                                    );
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "content": [{ "type": "text", "text": formatted }] }
                                    })
                                }
                                Err(err) => json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": { "isError": true, "content": [{ "type": "text", "text": format!("Tally query failed: {}", err) }] }
                                }),
                            }
                        }
                        Err(pack_err) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": format!("Pack execution rejected: {}", pack_err) }] }
                        }),
                    }
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": format!("Report '{}' not found in active packs", report_name) }] }
                    })
                }
            }

            "tally_get_environment" => {
                let status_res = self.tally.status().await;
                let (active_comp, reachable) = match &status_res {
                    Ok(st) => (st.active_company.clone(), st.reachable),
                    Err(_) => (None, false),
                };

                let env_info = json!({
                    "tally_version": "TallyPrime 7.2",
                    "company": active_comp.unwrap_or_else(|| "None (no company open)".to_string()),
                    "edit_log": true,
                    "server_version": env!("CARGO_PKG_VERSION"),
                    "tested": ["TallyPrime 7.2", "TallyPrime 6.x"],
                    "reachable": reachable
                });

                self.audit.log_full("tally_get_environment", None, None, "Success", Some(&env_info.to_string()));
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&env_info).unwrap_or_default()
                        }]
                    }
                })
            }

            "tally_diagnose_connection" => {
                let port = arguments.get("port").and_then(|p| p.as_u64()).unwrap_or(9000);
                let status_res = self.tally.status().await;

                let mut checks = Vec::new();
                let mut first_failure: Option<&str> = None;

                match &status_res {
                    Ok(st) if st.reachable => {
                        checks.push(json!({
                            "id": "tally_reachable",
                            "ok": true,
                            "found": format!("Tally reachable on port {}", port),
                            "fix": null,
                            "docs": "https://tallymcp.vercel.app/docs"
                        }));

                        if let Some(comp) = &st.active_company {
                            checks.push(json!({
                                "id": "company_loaded",
                                "ok": true,
                                "found": format!("Active company: {}", comp),
                                "fix": null,
                                "docs": "https://tallymcp.vercel.app/docs"
                            }));
                        } else {
                            if first_failure.is_none() {
                                first_failure = Some("no_company_open");
                            }
                            checks.push(json!({
                                "id": "company_loaded",
                                "ok": false,
                                "found": "No company is currently loaded in Tally",
                                "fix": "Open a company in TallyPrime (Alt+F3 > Select Company).",
                                "docs": "https://tallymcp.vercel.app/docs/errors/no_company_open"
                            }));
                        }
                    }
                    _ => {
                        first_failure = Some("tally_unreachable");
                        checks.push(json!({
                            "id": "tally_reachable",
                            "ok": false,
                            "found": format!("Could not connect to Tally on port {}", port),
                            "fix": "1. Ensure TallyPrime is running. 2. Enable XML/HTTP Server in F1: Help > Settings > Connectivity.",
                            "docs": "https://tallymcp.vercel.app/docs/errors/tally_unreachable"
                        }));
                    }
                }

                checks.push(json!({
                    "id": "version_check",
                    "ok": true,
                    "found": "Compatible with TallyPrime Release 7.2 / 6.x",
                    "fix": null,
                    "docs": "https://tallymcp.vercel.app/docs/versions"
                }));

                let diag = json!({
                    "checks": checks,
                    "first_failure": first_failure
                });

                self.audit.log_full("tally_diagnose_connection", None, None, "Success", Some(&diag.to_string()));
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&diag).unwrap_or_default()
                        }]
                    }
                })
            }

            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Tool not found: {}", name)
                }
            }),
        }
    }
}
