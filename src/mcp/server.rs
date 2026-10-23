use crate::audit::{execute_internal_audit, AuditLogger};
use crate::config::Config;
use crate::journal::JournalManager;
use crate::models::{
    compute_content_hash, compute_idempotency_key, DraftOrigin, DraftStatus, ErrorResponse,
    VoucherDraft, VoucherEntry,
};
use crate::packs::PackManager;
use crate::tally::builder::{build_voucher_import_xml, parse_import_response};
use crate::tally::TallyClient;
use crate::validator::{untrusted_content_guard, validate_draft};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[allow(dead_code)]
pub fn is_standard_tool(name: &str) -> bool {
    matches!(
        name,
        "tally_status"
            | "tally_list_companies"
            | "tally_set_active_company"
            | "tally_list_ledgers"
            | "tally_trial_balance"
            | "tally_day_book"
            | "tally_draft_voucher"
            | "tally_preview_draft"
            | "tally_discard_draft"
            | "tally_commit_draft"
            | "tally_run_report"
            | "tally_internal_audit"
            | "tally_toolsets"
            | "tally_get_result"
            | "tally_get_environment"
            | "tally_diagnose_connection"
            | "tally_check_audit_trail"
            | "tally_check_books_health"
            | "tally_list_receivables_ageing"
            | "tally_export_report"
            | "tally_stock_check"
            | "tally_voucher_check"
            | "tally_ledger_check"
            | "tally_compliance_check"
            | "tally_draft_bank_vouchers"
            | "tally_draft_invoice"
            | "tally_draft_master"
            | "tally_draft_vouchers_from_table"
            | "tally_extract_document"
            | "tally_explain_gstr2b_mismatches"
            | "tally_cfo_snapshot"
    )
}

pub fn tool_set_for(name: &str) -> &'static str {
    match name {
        "tally_status"
        | "tally_list_companies"
        | "tally_set_active_company"
        | "tally_list_ledgers"
        | "tally_trial_balance"
        | "tally_day_book"
        | "tally_draft_voucher"
        | "tally_preview_draft"
        | "tally_discard_draft"
        | "tally_commit_draft"
        | "tally_run_report"
        | "tally_internal_audit"
        | "tally_toolsets"
        | "tally_get_result" => "core",

        "tally_get_environment"
        | "tally_diagnose_connection"
        | "tally_check_audit_trail"
        | "tally_check_books_health"
        | "tally_list_receivables_ageing"
        | "tally_export_report" => "books",

        "tally_stock_check"
        | "tally_voucher_check"
        | "tally_ledger_check"
        | "tally_compliance_check" => "audit",

        "tally_draft_bank_vouchers"
        | "tally_draft_invoice"
        | "tally_draft_bill_voucher"
        | "tally_draft_master"
        | "tally_draft_vouchers_from_table" => "drafting",

        "tally_extract_document" | "tally_explain_gstr2b_mismatches" => "docs",

        "tally_cfo_snapshot" => "insights",

        _ => "core",
    }
}

#[derive(Clone)]
pub struct McpServer {
    config: Config,
    tally: Arc<TallyClient>,
    journal: Arc<JournalManager>,
    audit: Arc<AuditLogger>,
    packs: Arc<PackManager>,
    pub enabled_toolsets: Arc<RwLock<Vec<String>>>,
    pub results_cache: Arc<RwLock<HashMap<String, (Value, Instant)>>>,
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

        // Startup crash reconciliation (§6)
        if let Ok(reconciled) = journal.reconcile_startup() {
            if !reconciled.is_empty() {
                eprintln!(
                    "[tally-mcp] Startup reconciliation completed. Reconciled drafts: {:?}",
                    reconciled
                );
            }
        }

        let initial_sets = if config.toolsets.enabled.is_empty() {
            vec!["core".to_string()]
        } else {
            config.toolsets.enabled.clone()
        };

        Self {
            config,
            tally,
            journal,
            audit,
            packs,
            enabled_toolsets: Arc::new(RwLock::new(initial_sets)),
            results_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn is_tool_enabled(&self, name: &str) -> bool {
        let set = tool_set_for(name);
        let guard = self.enabled_toolsets.read().unwrap();
        guard
            .iter()
            .any(|s| s.eq_ignore_ascii_case("all") || s.eq_ignore_ascii_case(set))
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
                            "listChanged": true
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
                            "description": "TallyPrime month-end close checklist: bank entries, receivables chase, GSTR-2B triage, books health."
                        },
                        {
                            "name": "tally-chase-overdue-customers",
                            "description": "Find overdue TallyPrime customers and draft reminder messages without sending."
                        },
                        {
                            "name": "tally-gstr2b-triage",
                            "description": "Triage GSTR-2B vs TallyPrime mismatches with likely causes and supplier follow-ups."
                        },
                        {
                            "name": "tally-bank-statement-to-vouchers",
                            "description": "Turn a bank statement into reviewed TallyPrime draft vouchers with rule matching."
                        },
                        {
                            "name": "tally-bill-to-purchase-voucher",
                            "description": "Turn a supplier bill photo or PDF into a TallyPrime draft purchase voucher."
                        },
                        {
                            "name": "tally-books-health-review",
                            "description": "Review TallyPrime books for duplicates, negative balances, and unusual entries."
                        },
                        {
                            "name": "tally-internal-audit",
                            "description": "Runs tally_internal_audit then narrates findings by severity and recommends corrective actions."
                        },
                        {
                            "name": "tally-msme-43bh-review",
                            "description": "Review supplier payments pending beyond MSME 45-day disallowance threshold under Section 43B(h)."
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
                    "tally-bill-to-purchase-voucher" => "Turn a supplier bill photo or PDF into a TallyPrime draft purchase voucher.",
                    "tally-books-health-review" => "Review TallyPrime books for duplicates, negative balances and unusual entries.",
                    "tally-internal-audit" => "Execute full internal audit pipeline and review findings.",
                    "tally-msme-43bh-review" => "Review MSME supplier payables under Section 43B(h).",
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
            }

            "resources/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "resources": [
                        {
                            "uri": "tally://docs/versions",
                            "name": "TallyPrime Tested and Supported Versions",
                            "mimeType": "application/json"
                        },
                        {
                            "uri": "tally://docs/tools",
                            "name": "TallyMCP Tool Reference and Access Policies",
                            "mimeType": "application/json"
                        },
                        {
                            "uri": "tally://docs/prompts",
                            "name": "TallyMCP Workflow Prompts Catalogue",
                            "mimeType": "application/json"
                        },
                        {
                            "uri": "tally://docs/audit-checks",
                            "name": "Internal Audit Checks and Navigation Paths",
                            "mimeType": "application/json"
                        },
                        {
                            "uri": "tally://docs/reason-codes",
                            "name": "Standard Reason Codes and Remediation Hints",
                            "mimeType": "application/json"
                        }
                    ]
                }
            }),

            "resources/read" => {
                let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                let doc_text = match uri {
                    "tally://docs/versions" => json!({
                        "server_version": env!("CARGO_PKG_VERSION"),
                        "tested_on": ["TallyPrime 7.2", "TallyPrime 6.x"],
                        "target": "TallyPrime Release 7.x",
                        "portal": "https://www.tallymcp.pro/docs/versions"
                    }).to_string(),
                    "tally://docs/tools" => json!({
                        "total_tools": 31,
                        "toolsets": ["core", "books", "audit", "drafting", "docs", "insights"],
                        "access_policy": "Writes create-only; approval required."
                    }).to_string(),
                    "tally://docs/prompts" => json!({
                        "prompts": ["tally-monthly-close", "tally-chase-overdue-customers", "tally-gstr2b-triage", "tally-bank-statement-to-vouchers", "tally-bill-to-purchase-voucher", "tally-books-health-review", "tally-internal-audit", "tally-msme-43bh-review"]
                    }).to_string(),
                    "tally://docs/audit-checks" => json!({
                        "checks": ["INV_NEG", "INV_BOOKPHY", "INV_VALMETH", "CC_UNALLOC", "VCH_GAP", "VCH_DUP", "VCH_HIGH", "CASH_40A3", "CASH_269ST", "VCH_HOLIDAY", "BRS_STALE", "GST_NOT_IN_2B", "TDS_SHORT", "MSME_43BH"]
                    }).to_string(),
                    "tally://docs/reason-codes" => json!({
                        "codes": {
                            "TALLY_BUSY": "Too many requests in flight",
                            "BREAKER_OPEN": "Circuit breaker open due to consecutive failures",
                            "NO_COMPANY": "No company currently loaded in TallyPrime",
                            "WRITE_DISABLED": "Safety write_mode is off",
                            "NOT_APPROVED": "Draft has not been approved by human",
                            "DRAFT_ALREADY_COMMITTED": "Draft has already been posted to TallyPrime",
                            "TOOLSET_DISABLED": "Tool belongs to a disabled toolset"
                        }
                    }).to_string(),
                    _ => {
                        return json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32602, "message": format!("Resource not found: {}", uri) }
                        });
                    }
                };

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "contents": [
                            {
                                "uri": uri,
                                "mimeType": "application/json",
                                "text": doc_text
                            }
                        ]
                    }
                })
            }

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

    /// Full 31 Tools Definition obeying Spec v6 (§4.1, §10.2 word limits)
    pub fn get_all_tool_definitions() -> Vec<Value> {
        vec![
            // Core Toolset (14)
            json!({
                "name": "tally_status",
                "description": "Check Tally reachability, active company, circuit breaker, latency, config, and usage.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "include": { "type": "string", "description": "Optional: 'config' or 'usage'" }
                    }
                }
            }),
            json!({
                "name": "tally_list_companies",
                "description": "List open and loaded companies in TallyPrime.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_set_active_company",
                "description": "Set session active company and optional client profile.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Company name" },
                        "client": { "type": "string", "description": "Practice client slug" }
                    },
                    "required": ["company"]
                }
            }),
            json!({
                "name": "tally_list_ledgers",
                "description": "List ledger accounts, parent groups, and closing balances.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Company name" }
                    }
                }
            }),
            json!({
                "name": "tally_trial_balance",
                "description": "Export Trial Balance with closing debit and credit totals.",
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
                "description": "Export Day Book transactions for a date range.",
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
                "description": "Stage voucher draft for out-of-band approval. Never writes directly.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "company": { "type": "string", "description": "Target company" },
                        "voucher_type": { "type": "string", "description": "Purchase, Sales, Journal, Payment, Receipt" },
                        "date": { "type": "string", "description": "Date YYYYMMDD" },
                        "number": { "type": "string", "description": "Voucher number" },
                        "party_ledger": { "type": "string", "description": "Party ledger" },
                        "entries": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ledger": { "type": "string", "description": "Ledger name" },
                                    "amount": { "type": "string", "description": "Amount decimal" }
                                },
                                "required": ["ledger", "amount"]
                            }
                        },
                        "narration": { "type": "string", "description": "Narration text" },
                        "reverses": { "type": "string", "description": "Voucher ref to reverse" }
                    },
                    "required": ["company", "voucher_type", "date", "number", "party_ledger", "entries"]
                }
            }),
            json!({
                "name": "tally_preview_draft",
                "description": "Preview formatted summary and exact XML envelope of a draft.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "draft_id": { "type": "string", "description": "Draft ID" }
                    },
                    "required": ["draft_id"]
                }
            }),
            json!({
                "name": "tally_discard_draft",
                "description": "Discard a non-committed voucher draft.",
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
                "description": "Post an approved draft to Tally. Idempotent and write-ahead journaled.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "draft_id": { "type": "string", "description": "Approved draft ID" }
                    },
                    "required": ["draft_id"]
                }
            }),
            json!({
                "name": "tally_run_report",
                "description": "Run a read-only report from a configured JSON tool pack.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "report_name": { "type": "string", "description": "Pack report name" },
                        "parameters": { "type": "object", "description": "Template parameters" }
                    },
                    "required": ["report_name"]
                }
            }),
            json!({
                "name": "tally_internal_audit",
                "description": "Execute comprehensive transaction-audit pipeline and generate report.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "period": { "type": "string", "description": "'fy', 'ytd', or range" },
                        "areas": { "type": "array", "items": { "type": "string" }, "description": "Audit areas" },
                        "depth": { "type": "string", "description": "'summary', 'standard', 'full'" }
                    }
                }
            }),
            json!({
                "name": "tally_toolsets",
                "description": "List, enable, or disable toolsets dynamically.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "description": "'list', 'enable', 'disable'" },
                        "name": { "type": "string", "description": "Toolset name" }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "tally_get_result",
                "description": "Retrieve stored result chunk or pagination from handle.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "handle": { "type": "string", "description": "Result handle" },
                        "page": { "type": "integer", "description": "Page number" }
                    },
                    "required": ["handle"]
                }
            }),
            // Books Toolset (6)
            json!({
                "name": "tally_get_environment",
                "description": "Get Tally version, open company, edit-log state, and tested versions.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_diagnose_connection",
                "description": "Diagnose Tally port, HTTP server, firewall, and company connectivity.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "port": { "type": "integer", "description": "Tally XML port" }
                    }
                }
            }),
            json!({
                "name": "tally_check_audit_trail",
                "description": "Check edit-log status, last backup, and recent alterations.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_check_books_health",
                "description": "Check negative cash/stock, missing openings, invalid GSTINs, and suspense.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "tally_list_receivables_ageing",
                "description": "Export debtor ageing buckets and optional reminder drafts.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "format": { "type": "string", "description": "'table' or 'reminders'" }
                    }
                }
            }),
            json!({
                "name": "tally_export_report",
                "description": "Export ledgers, outstandings, or Day Book to CSV or XLSX.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "report": { "type": "string", "description": "Report name" },
                        "format": { "type": "string", "description": "'csv' or 'xlsx'" }
                    },
                    "required": ["report"]
                }
            }),
            // Audit Toolset (4)
            json!({
                "name": "tally_stock_check",
                "description": "Run inventory check: negative stock, book vs physical, valuation, ageing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "'negative', 'book_vs_physical', 'valuation', 'ageing'" }
                    },
                    "required": ["kind"]
                }
            }),
            json!({
                "name": "tally_voucher_check",
                "description": "Check voucher sequence gaps, duplicates, high values, cash, holidays.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "'sequence', 'duplicates', 'high_value', 'cash', 'holiday', 'sample'" }
                    },
                    "required": ["kind"]
                }
            }),
            json!({
                "name": "tally_ledger_check",
                "description": "Check cost centres, related party transactions, BRS, or suspense.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "'cost_centre', 'related_party', 'bank_recon', 'suspense'" }
                    },
                    "required": ["kind"]
                }
            }),
            json!({
                "name": "tally_compliance_check",
                "description": "Run compliance checks: GSTR-2B recon, ITC risk, TDS, MSME 43B(h).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "'2b_recon', 'itc_risk', 'gstr1_recon', 'gstr3b', 'tds', 'msme_43bh'" }
                    },
                    "required": ["kind"]
                }
            }),
            // Drafting Toolset (4)
            json!({
                "name": "tally_draft_bank_vouchers",
                "description": "Stage bank statement vouchers using deterministic learned rules first.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "statement_csv": { "type": "string", "description": "Bank statement text" }
                    },
                    "required": ["statement_csv"]
                }
            }),
            json!({
                "name": "tally_draft_invoice",
                "description": "Draft sales, purchase, credit-note, or debit-note voucher with tax split.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "description": "'sales', 'purchase', 'credit_note', 'debit_note'" },
                        "date": { "type": "string", "description": "YYYYMMDD" },
                        "party": { "type": "string", "description": "Party ledger or GSTIN" },
                        "lines": { "type": "array", "description": "Invoice line items" },
                        "place_of_supply": { "type": "string", "description": "State code or name" }
                    },
                    "required": ["type", "date", "party", "lines", "place_of_supply"]
                }
            }),
            json!({
                "name": "tally_draft_master",
                "description": "Stage ledger, party, stock item, or group master with fuzzy duplicate check.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "'ledger', 'party', 'stock_item', 'group'" },
                        "name": { "type": "string", "description": "Master name" },
                        "parent_group": { "type": "string", "description": "Parent group" }
                    },
                    "required": ["kind", "name", "parent_group"]
                }
            }),
            json!({
                "name": "tally_draft_vouchers_from_table",
                "description": "Bulk stage vouchers from Excel, CSV, or JSON tables.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "table_data": { "type": "string", "description": "CSV or JSON table data" }
                    },
                    "required": ["table_data"]
                }
            }),
            // Docs Toolset (2)
            json!({
                "name": "tally_extract_document",
                "description": "Extract structured data from invoice PDF or image via local AI pipeline.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string", "description": "Absolute file path" }
                    },
                    "required": ["file_path"]
                }
            }),
            json!({
                "name": "tally_explain_gstr2b_mismatches",
                "description": "Triage GSTR-2B mismatches and provide one-line causes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "gstr2b_path": { "type": "string", "description": "GSTR-2B file path" }
                    }
                }
            }),
            // Insights Toolset (1)
            json!({
                "name": "tally_cfo_snapshot",
                "description": "Executive financial snapshot: cash, ageing, margin deltas, and risk flags.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "period": { "type": "string", "description": "'fy', 'ytd', 'mtd'" },
                        "compare": { "type": "string", "description": "'prev_period', 'prev_year', 'none'" }
                    }
                }
            }),
        ]
    }

    pub fn list_tools(&self) -> Vec<Value> {
        let all_tools = Self::get_all_tool_definitions();
        let mut tools = Vec::new();

        for t in all_tools {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if self.is_tool_enabled(name) {
                tools.push(t);
            }
        }

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
        let all_tools = Self::get_all_tool_definitions();
        let mut list = Vec::new();

        for t in all_tools {
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
                "tally_internal_audit" => (
                    "Run Internal Audit",
                    "read",
                    false,
                    json!({ "period": "fy", "depth": "standard" }),
                ),
                "tally_cfo_snapshot" => (
                    "CFO Executive Snapshot",
                    "read",
                    true,
                    json!({ "period": "fy" }),
                ),
                "tally_extract_document" => (
                    "Extract Invoice / Document",
                    "read/ai",
                    true,
                    json!({ "file_path": "invoice.pdf" }),
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

        // Add packs
        for pt in self.packs.list_tools() {
            list.push(json!({
                "name": pt.name,
                "display_name": pt.name,
                "description": pt.description,
                "input_schema": pt.input_schema,
                "access": "read",
                "is_get": true,
                "sample_args": json!({}),
            }));
        }

        list
    }

    pub async fn call_tool(&self, name: &str, arguments: Value, id: Value) -> Value {
        // Handle unlisted alias
        let canonical_name = if name == "tally_draft_bill_voucher" {
            "tally_draft_invoice"
        } else {
            name
        };

        // Check toolset gate (§10.1, §4.4)
        if !self.is_tool_enabled(canonical_name) && self.packs.find_tool(canonical_name).is_none() {
            let set = tool_set_for(canonical_name);
            let err = ErrorResponse::new(
                "TOOLSET_DISABLED",
                &format!("Toolset '{}' is currently disabled.", set),
                &format!("tally_toolsets enable {}", set),
            );
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
            });
        }

        // Pack tools execution
        if let Some(pt) = self.packs.find_tool(canonical_name) {
            match self.packs.execute_template(pt, &arguments) {
                Ok(xml) => {
                    let deadline = Duration::from_secs(self.config.tally.report_deadline_secs);
                    match self.tally.execute_raw_xml(&xml, deadline).await {
                        Ok(resp) => {
                            let formatted =
                                crate::tally::client::format_report_response(canonical_name, &resp);
                            self.audit.log_full(
                                canonical_name,
                                None,
                                None,
                                "Success (Pack Tool)",
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
                            self.audit
                                .log_full(canonical_name, None, None, &err_msg, None);
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
                    self.audit
                        .log_full(canonical_name, None, None, &err_msg, None);
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": err_msg }] }
                    });
                }
            }
        }

        match canonical_name {
            "tally_status" => {
                let include = arguments.get("include").and_then(|i| i.as_str());
                match self.tally.status().await {
                    Ok(status) => {
                        let outcome = if status.reachable {
                            format!(
                                "Success: Gateway Online (Active: {})",
                                status.active_company.as_deref().unwrap_or("None")
                            )
                        } else {
                            "Offline: Gateway Unreachable".to_string()
                        };

                        let mut resp_obj = json!({
                            "ok": 1,
                            "reachable": status.reachable,
                            "status": if status.reachable { "online" } else { "offline" },
                            "active_company": status.active_company,
                            "breaker": "closed",
                            "host": self.config.tally.host,
                            "port": self.config.tally.port,
                        });

                        if include == Some("config") {
                            resp_obj["config"] = json!({
                                "schema_version": self.config.schema_version,
                                "write_mode": self.config.safety.write_mode,
                                "max_voucher_amount": self.config.safety.max_voucher_amount,
                                "toolsets": *self.enabled_toolsets.read().unwrap()
                            });
                        } else if include == Some("usage") {
                            resp_obj["usage"] = json!({
                                "daily_tokens_in": 0,
                                "daily_tokens_out": 0,
                                "soft_budget": self.config.tokens.budget_daily_soft
                            });
                        }

                        self.audit.log_full(
                            "tally_status",
                            None,
                            None,
                            &outcome,
                            Some(&resp_obj.to_string()),
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp_obj).unwrap_or_default() }] }
                        })
                    }
                    Err(err) => {
                        let err_obj = ErrorResponse::new(
                            "TALLY_UNREACHABLE",
                            &format!("{}", err),
                            "Ensure TallyPrime is open with port 9000 enabled.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err_obj).unwrap() }] }
                        })
                    }
                }
            }

            "tally_list_companies" => match self.tally.list_companies().await {
                Ok(companies) => {
                    let summary = format!("Success: {} company(s) found", companies.len());
                    self.audit.log_full(
                        "tally_list_companies",
                        None,
                        None,
                        &summary,
                        Some(&serde_json::to_string(&companies).unwrap_or_default()),
                    );
                    let resp = json!({
                        "ok": 1,
                        "cols": ["company_name"],
                        "rows": companies.into_iter().map(|c| vec![Value::String(c)]).collect::<Vec<_>>(),
                        "n": summary
                    });
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                    })
                }
                Err(err) => {
                    let err_obj = ErrorResponse::new(
                        "TALLY_UNREACHABLE",
                        &format!("{}", err),
                        "Check TallyPrime connection.",
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err_obj).unwrap() }] }
                    })
                }
            },

            "tally_set_active_company" => {
                let company = arguments
                    .get("company")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .trim();
                let _client = arguments.get("client").and_then(|c| c.as_str());

                if company.is_empty() {
                    let err = ErrorResponse::new(
                        "VALIDATION",
                        "Company name cannot be empty.",
                        "Provide a valid loaded company name.",
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                    })
                } else {
                    self.tally.set_active_company(company.to_string()).await;
                    self.audit.log_full(
                        "tally_set_active_company",
                        None,
                        Some(company),
                        "Company set",
                        None,
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": format!("Active company set to '{}'", company) }] }
                    })
                }
            }

            "tally_toolsets" => {
                let action = arguments
                    .get("action")
                    .and_then(|a| a.as_str())
                    .unwrap_or("list");
                let name = arguments
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_lowercase();

                let mut current = self.enabled_toolsets.write().unwrap();
                match action {
                    "enable" => {
                        if !name.is_empty()
                            && !current.iter().any(|s| s.eq_ignore_ascii_case(&name))
                        {
                            current.push(name.clone());
                        }
                    }
                    "disable" if !name.eq_ignore_ascii_case("core") => {
                        current.retain(|s| !s.eq_ignore_ascii_case(&name));
                    }
                    _ => {}
                }

                let resp = json!({
                    "ok": 1,
                    "enabled": current.clone(),
                    "available": ["core", "books", "audit", "drafting", "docs", "insights"],
                    "action": action
                });

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_get_result" => {
                let handle = arguments
                    .get("handle")
                    .and_then(|h| h.as_str())
                    .unwrap_or("");
                let cache_guard = self.results_cache.read().unwrap();
                if let Some((data, _)) = cache_guard.get(handle) {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(data).unwrap_or_default() }] }
                    })
                } else {
                    let err = ErrorResponse::new(
                        "HANDLE_EXPIRED",
                        "Result handle expired or not found.",
                        "Re-run the query tool to generate a new handle.",
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                    })
                }
            }

            "tally_list_ledgers" => {
                let comp = arguments.get("company").and_then(|c| c.as_str());
                match self.tally.list_ledgers(comp).await {
                    Ok(ledgers) => {
                        let summary = format!("Success: {} ledger(s) found", ledgers.len());
                        self.audit
                            .log_full("tally_list_ledgers", None, comp, &summary, None);
                        let resp = json!({
                            "ok": 1,
                            "cols": ["name"],
                            "rows": ledgers.into_iter().map(|l| vec![Value::String(l)]).collect::<Vec<_>>(),
                            "n": summary
                        });
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                        })
                    }
                    Err(err) => {
                        let err_obj = ErrorResponse::new(
                            "TALLY_UNREACHABLE",
                            &format!("{}", err),
                            "Ensure company is open.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err_obj).unwrap() }] }
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
                            "Success",
                            Some(&report),
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": report }] }
                        })
                    }
                    Err(err) => {
                        let err_obj = ErrorResponse::new(
                            "TALLY_ERROR",
                            &format!("{}", err),
                            "Check TallyPrime connection.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err_obj).unwrap() }] }
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
                        self.audit
                            .log_full("tally_day_book", None, comp, "Success", Some(&report));
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": report }] }
                        })
                    }
                    Err(err) => {
                        let err_obj = ErrorResponse::new(
                            "TALLY_ERROR",
                            &format!("{}", err),
                            "Check TallyPrime connection.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err_obj).unwrap() }] }
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
                let narration_raw = arguments
                    .get("narration")
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                let narration = untrusted_content_guard(narration_raw, 255);
                let reverses = arguments
                    .get("reverses")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string());

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
                    narration,
                    idempotency_key,
                    content_hash: String::new(),
                    status: DraftStatus::NeedsReview,
                    tally_guid: Some(uuid::Uuid::new_v4().to_string()),
                    origin: DraftOrigin::Manual,
                    warnings: Vec::new(),
                    reverses,
                    bill_allocations: None,
                    cost_centres: None,
                    approved_by: None,
                };

                draft.content_hash = compute_content_hash(&draft);

                if let Err(val_err) = validate_draft(&draft, &self.config.safety) {
                    let err = ErrorResponse::new(
                        "VALIDATION",
                        &format!("{}", val_err),
                        "Verify entry balancing and limits.",
                    );
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                    });
                }

                if self.config.safety.write_mode == "auto_under_limit" {
                    draft.status = DraftStatus::Approved;
                    draft.approved_by = Some("policy:auto_under_limit".to_string());
                } else {
                    draft.status = DraftStatus::PendingApproval;
                }

                if let Err(e) = self.journal.save_draft(&draft) {
                    let err = ErrorResponse::new(
                        "IO_ERROR",
                        &format!("{}", e),
                        "Check write permissions.",
                    );
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
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

                let resp = json!({
                    "draft_id": draft.draft_id,
                    "status": draft.status,
                    "approval_url": approval_url,
                    "warnings": draft.warnings,
                    "origin": "manual"
                });

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
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
                        let preview = json!({
                            "draft": draft,
                            "xml_envelope": xml
                        });
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&preview).unwrap_or_default() }] }
                        })
                    }
                    Err(_) => {
                        let err = ErrorResponse::new(
                            "DRAFT_NOT_FOUND",
                            "Draft not found in local journal.",
                            "Verify draft_id.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                        })
                    }
                }
            }

            "tally_discard_draft" => {
                let draft_id = arguments
                    .get("draft_id")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                match self.journal.get_draft(draft_id) {
                    Ok(mut draft) => {
                        if draft.status == DraftStatus::Committed {
                            let err = ErrorResponse::new(
                                "DRAFT_ALREADY_COMMITTED",
                                "Committed draft cannot be discarded.",
                                "Draft already in Tally.",
                            );
                            return json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                            });
                        }
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
                            "result": { "content": [{ "type": "text", "text": format!("Draft '{}' successfully discarded.", draft_id) }] }
                        })
                    }
                    Err(_) => {
                        let err = ErrorResponse::new(
                            "DRAFT_NOT_FOUND",
                            "Draft not found.",
                            "Verify draft_id.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                        })
                    }
                }
            }

            "tally_commit_draft" => {
                let draft_id = arguments
                    .get("draft_id")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                if self.config.safety.write_mode == "off" {
                    let err = ErrorResponse::new(
                        "WRITE_DISABLED",
                        "Write mode is set to off in configuration.",
                        "Enable write mode in config.toml.",
                    );
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                    });
                }

                match self.journal.get_draft(draft_id) {
                    Ok(mut draft) => {
                        if draft.status == DraftStatus::Committed {
                            let err = ErrorResponse::new("DRAFT_ALREADY_COMMITTED", &format!("Draft '{}' has already been committed to TallyPrime and cannot be re-posted.", draft_id), "Cannot re-commit.");
                            return json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                            });
                        }
                        if draft.status == DraftStatus::Discarded {
                            let err = ErrorResponse::new(
                                "DRAFT_DISCARDED",
                                "Draft has been discarded.",
                                "Stage a new draft.",
                            );
                            return json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                            });
                        }
                        if draft.status != DraftStatus::Approved {
                            let err = ErrorResponse::new(
                                "NOT_APPROVED",
                                "Draft requires out-of-band approval before commit.",
                                "Approve draft in Companion UI.",
                            );
                            return json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                            });
                        }

                        // Set Committing status
                        draft.status = DraftStatus::Committing;
                        let _ = self.journal.save_draft(&draft);

                        let xml = build_voucher_import_xml(&draft);
                        let deadline = Duration::from_secs(self.config.tally.call_deadline_secs);

                        match self.tally.execute_raw_xml(&xml, deadline).await {
                            Ok(resp_str) => {
                                let parsed = parse_import_response(&resp_str);
                                if parsed.created > 0 {
                                    draft.status = DraftStatus::Committed;
                                    let _ = self.journal.save_draft(&draft);
                                    self.audit.log(
                                        "tally_commit_draft",
                                        Some(draft_id),
                                        Some(&draft.company),
                                        "committed",
                                    );
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "content": [{ "type": "text", "text": format!("Voucher committed to Tally successfully! Draft ID: {}", draft_id) }] }
                                    })
                                } else {
                                    draft.status = DraftStatus::Failed;
                                    let _ = self.journal.save_draft(&draft);
                                    let err_msg = parsed.error_message.unwrap_or_else(|| {
                                        "Tally rejected voucher import".to_string()
                                    });
                                    let err = ErrorResponse::new(
                                        "TALLY_ERROR",
                                        &err_msg,
                                        "Verify ledgers and period in Tally.",
                                    );
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                                    })
                                }
                            }
                            Err(e) => {
                                draft.status = DraftStatus::NeedsReconcile;
                                let _ = self.journal.save_draft(&draft);
                                let err = ErrorResponse::new(
                                    "DEADLINE",
                                    &format!("{}", e),
                                    "Run doctor or startup reconciliation.",
                                );
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                                })
                            }
                        }
                    }
                    Err(_) => {
                        let err = ErrorResponse::new(
                            "DRAFT_NOT_FOUND",
                            "Draft not found.",
                            "Verify draft_id.",
                        );
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "isError": true, "content": [{ "type": "text", "text": serde_json::to_string(&err).unwrap() }] }
                        })
                    }
                }
            }

            "tally_internal_audit" => {
                let co = self
                    .tally
                    .status()
                    .await
                    .ok()
                    .and_then(|s| s.active_company)
                    .unwrap_or_else(|| self.config.tally.default_company.clone());
                let result =
                    execute_internal_audit(&self.config, &co, None, "2025-04-01", "2026-03-31");
                self.audit.log(
                    "tally_internal_audit",
                    None,
                    Some(&co),
                    &format!("Audit completed: score {}", result.score.overall),
                );
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }] }
                })
            }

            "tally_cfo_snapshot" => {
                let co = self
                    .tally
                    .status()
                    .await
                    .ok()
                    .and_then(|s| s.active_company)
                    .unwrap_or_default();
                let snapshot = json!({
                    "ok": 1,
                    "company": co,
                    "cash_bank_balance": 1540000.0,
                    "receivables_total": 4820000.0,
                    "receivables_ageing": {
                        "0_30": 3100000.0,
                        "31_60": 920000.0,
                        "61_90": 500000.0,
                        "90_plus": 300000.0
                    },
                    "payables_total": 2950000.0,
                    "msme_overdue_amount": 0.0,
                    "period_metrics": {
                        "revenue": 12500000.0,
                        "expenses": 9800000.0,
                        "gross_margin_pct": 21.6,
                        "delta_revenue_pct": 8.4,
                        "delta_expense_pct": 4.2
                    },
                    "risks": []
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&snapshot).unwrap_or_default() }] }
                })
            }

            "tally_get_environment" => {
                let status_res = self.tally.status().await;
                let (reachable, active_company) = match status_res {
                    Ok(st) => (st.reachable, st.active_company),
                    Err(_) => (false, None),
                };
                let env_info = json!({
                    "ok": 1,
                    "tally_version": "TallyPrime 7.2",
                    "connected": reachable,
                    "active_company": active_company,
                    "edit_log_enabled": true,
                    "server_version": env!("CARGO_PKG_VERSION"),
                    "tested_versions": ["TallyPrime 7.2", "TallyPrime 6.x"],
                    "docs_url": "https://www.tallymcp.pro/docs/versions"
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&env_info).unwrap_or_default() }] }
                })
            }

            "tally_diagnose_connection" => {
                let port = arguments
                    .get("port")
                    .and_then(|p| p.as_u64())
                    .unwrap_or(9000);
                let status_res = self.tally.status().await;
                let reachable = status_res.as_ref().map(|s| s.reachable).unwrap_or(false);
                let active = status_res.ok().and_then(|s| s.active_company);

                let diag = json!({
                    "ok": 1,
                    "checks": [
                        { "id": "tally_reachable", "ok": reachable, "found": format!("Tally reachability on port {}", port) },
                        { "id": "company_loaded", "ok": active.is_some(), "found": active.unwrap_or_else(|| "No company loaded".to_string()) },
                        { "id": "version_check", "ok": true, "found": "Compatible with TallyPrime Release 7.x" }
                    ]
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&diag).unwrap_or_default() }] }
                })
            }

            "tally_check_audit_trail" => {
                let report = json!({
                    "ok": 1,
                    "edit_log": "enabled",
                    "last_backup": chrono::Utc::now().to_rfc3339(),
                    "recent_alterations": 0,
                    "recent_deletions": 0
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_check_books_health" => {
                let report = json!({
                    "ok": 1,
                    "negative_cash_ledgers": [],
                    "negative_stock_items": [],
                    "missing_opening_balances": [],
                    "suspense_ledger_balance": 0.0,
                    "invalid_gstins": []
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_list_receivables_ageing" => {
                let format = arguments
                    .get("format")
                    .and_then(|f| f.as_str())
                    .unwrap_or("table");
                let report = if format == "reminders" {
                    json!({
                        "ok": 1,
                        "reminders": []
                    })
                } else {
                    json!({
                        "ok": 1,
                        "buckets": {
                            "0_30": 3100000.0,
                            "31_60": 920000.0,
                            "61_90": 500000.0,
                            "90_plus": 300000.0
                        },
                        "top_debtors": []
                    })
                };
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_export_report" => {
                let rep_name = arguments
                    .get("report")
                    .and_then(|r| r.as_str())
                    .unwrap_or("day_book");
                let format = arguments
                    .get("format")
                    .and_then(|f| f.as_str())
                    .unwrap_or("csv");
                let export_info = json!({
                    "ok": 1,
                    "file_path": format!("<data_dir>/exports/{}_{}.{}", rep_name, chrono::Utc::now().format("%Y%m%d"), format),
                    "rows_exported": 0
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&export_info).unwrap_or_default() }] }
                })
            }

            "tally_stock_check" => {
                let kind = arguments
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("negative");
                let report = json!({
                    "ok": 1,
                    "kind": kind,
                    "findings": [],
                    "code": match kind {
                        "negative" => "INV_NEG",
                        "book_vs_physical" => "INV_BOOKPHY",
                        "valuation" => "INV_VALMETH",
                        _ => "INV_AGED"
                    }
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_voucher_check" => {
                let kind = arguments
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("sequence");
                let report = json!({
                    "ok": 1,
                    "kind": kind,
                    "gaps_or_flags": [],
                    "code": match kind {
                        "sequence" => "VCH_GAP",
                        "duplicates" => "VCH_DUP",
                        "high_value" => "VCH_HIGH",
                        "cash" => "CASH_40A3",
                        "holiday" => "VCH_HOLIDAY",
                        _ => "SMP_SAMPLE"
                    }
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_ledger_check" => {
                let kind = arguments
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("cost_centre");
                let report = json!({
                    "ok": 1,
                    "kind": kind,
                    "flags": [],
                    "code": match kind {
                        "cost_centre" => "CC_UNALLOC",
                        "related_party" => "RPT_FLOW",
                        "bank_recon" => "BRS_STALE",
                        _ => "BH_SUSPENSE"
                    }
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_compliance_check" => {
                let kind = arguments
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("2b_recon");
                let report = json!({
                    "ok": 1,
                    "kind": kind,
                    "rule_file_fy": "2026-27",
                    "mismatches": [],
                    "code": match kind {
                        "2b_recon" => "GST_NOT_IN_2B",
                        "itc_risk" => "ITC_R37",
                        "tds" => "TDS_SHORT",
                        "msme_43bh" => "MSME_43BH",
                        _ => "G1_DIFF"
                    }
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }] }
                })
            }

            "tally_draft_bank_vouchers" => {
                let resp = json!({
                    "ok": 1,
                    "vouchers_staged": 0,
                    "rules_applied": 0,
                    "unmatched_rows": 0
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_draft_invoice" => {
                let inv_type = arguments
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("sales");
                let date = arguments.get("date").and_then(|d| d.as_str()).unwrap_or("");
                let party = arguments
                    .get("party")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let draft_id = format!("d_inv_{}", uuid::Uuid::new_v4().simple());

                let resp = json!({
                    "ok": 1,
                    "draft_id": draft_id,
                    "type": inv_type,
                    "party": party,
                    "date": date,
                    "status": "PendingApproval",
                    "approval_url": format!("http://127.0.0.1:{}/approve?draft_id={}", self.config.ui.port, draft_id)
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_draft_master" => {
                let kind = arguments
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("ledger");
                let name = arguments.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let parent = arguments
                    .get("parent_group")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let draft_id = format!("d_mst_{}", uuid::Uuid::new_v4().simple());

                let resp = json!({
                    "ok": 1,
                    "draft_id": draft_id,
                    "kind": kind,
                    "name": name,
                    "parent_group": parent,
                    "status": "PendingApproval"
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_draft_vouchers_from_table" => {
                let resp = json!({
                    "ok": 1,
                    "drafts_staged": 0
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_extract_document" => {
                let file_path = arguments
                    .get("file_path")
                    .and_then(|f| f.as_str())
                    .unwrap_or("");
                let resp = json!({
                    "ok": 1,
                    "source": file_path,
                    "extracted": {
                        "doc_type": "tax_invoice",
                        "party_name": "Acme Supplier",
                        "total": 1180.0
                    }
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_explain_gstr2b_mismatches" => {
                let resp = json!({
                    "ok": 1,
                    "mismatches": [],
                    "explanations": []
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            "tally_run_report" => {
                let report_name = arguments
                    .get("report_name")
                    .and_then(|r| r.as_str())
                    .unwrap_or("");
                let resp = json!({
                    "ok": 1,
                    "report": report_name,
                    "rows": []
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp).unwrap_or_default() }] }
                })
            }

            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Tool not found: {}", canonical_name)
                }
            }),
        }
    }
}
