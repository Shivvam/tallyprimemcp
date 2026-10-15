use crate::audit::AuditLogger;
use crate::config::Config;
use crate::journal::JournalManager;
use crate::models::DraftStatus;
use crate::tally::builder::build_voucher_import_xml;
use crate::tally::TallyClient;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tiny_http::{Header, Response, Server};

const UI_HTML: &str = include_str!("assets/index.html");

pub struct CompanionUiServer {
    config: Config,
    tally: Arc<TallyClient>,
    journal: Arc<JournalManager>,
    audit: Arc<AuditLogger>,
    launch_token: String,
}

impl CompanionUiServer {
    pub fn new(config: Config) -> Self {
        let tally = Arc::new(TallyClient::new(config.tally.clone()));
        let journal = Arc::new(JournalManager::new().expect("Failed to initialize journal manager"));
        let audit = Arc::new(AuditLogger::new());
        let launch_token = format!("tok_{}", uuid::Uuid::new_v4().simple());

        Self {
            config,
            tally,
            journal,
            audit,
            launch_token,
        }
    }

    #[allow(dead_code)]
    pub fn launch_token(&self) -> &str {
        &self.launch_token
    }

    pub fn run(&self, port: u16) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("127.0.0.1:{}", port);
        let server = Server::http(&addr).map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

        println!("=== TallyMCP Companion UI Running ===");
        println!("URL: http://127.0.0.1:{}/?token={}", port, self.launch_token);
        println!("Press Ctrl+C to terminate the companion UI.");

        // Fast sync reachability check helper supporting both localhost and LAN IPs
        let check_tally_quick = |host: &str, port: u16| -> bool {
            use std::net::{TcpStream, ToSocketAddrs};
            let addr = format!("{}:{}", host, port);
            if let Ok(mut addrs) = addr.to_socket_addrs() {
                if let Some(sock_addr) = addrs.next() {
                    return TcpStream::connect_timeout(&sock_addr, Duration::from_millis(500)).is_ok();
                }
            }
            false
        };

        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let method = request.method().as_str().to_string();

            // Strict Host header verification against DNS rebinding
            let host_header = request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("host"));
            if let Some(h) = host_header {
                let h_val = h.value.as_str();
                if !h_val.starts_with("127.0.0.1") && !h_val.starts_with("localhost") {
                    let resp = Response::from_string("Forbidden: Host header not loopback").with_status_code(403);
                    let _ = request.respond(resp);
                    continue;
                }
            }

            if url == "/" || url.starts_with("/?token=") {
                let header = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
                let csp = Header::from_bytes(&b"Content-Security-Policy"[..], &b"default-src 'self' 'unsafe-inline'"[..]).unwrap();
                let resp = Response::from_string(UI_HTML).with_header(header).with_header(csp);
                let _ = request.respond(resp);
                continue;
            }

            if url == "/api/status" && method == "GET" {
                let reachable = check_tally_quick(&self.config.tally.host, self.config.tally.port);

                let payload = json!({
                    "reachable": reachable,
                    "target_url": self.tally.target_url(),
                    "circuit_breaker": "Closed",
                    "write_mode": self.config.safety.write_mode
                });

                let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(payload.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url == "/api/drafts" && method == "GET" {
                let drafts = self.journal.list_pending_drafts().unwrap_or_default();
                let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(serde_json::to_string(&drafts).unwrap_or_default()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url == "/api/drafts/approve" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request" });

                if let Ok(val) = parsed {
                    let draft_id = val.get("draft_id").and_then(|d| d.as_str()).unwrap_or("");
                    let content_hash = val.get("content_hash").and_then(|h| h.as_str()).unwrap_or("");

                    if let Ok(mut draft) = self.journal.get_draft(draft_id) {
                        if draft.content_hash == content_hash {
                            draft.status = DraftStatus::Approved;
                            if self.journal.save_draft(&draft).is_ok() {
                                self.audit.log("companion_ui_approve", Some(draft_id), Some(&draft.company), "human_approved");
                                outcome = json!({ "success": true, "draft_id": draft_id });
                            }
                        } else {
                            outcome = json!({ "success": false, "error": "Content hash mismatch. Draft was altered." });
                        }
                    }
                }

                let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url == "/api/drafts/commit" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request" });

                if let Ok(val) = parsed {
                    let draft_id = val.get("draft_id").and_then(|d| d.as_str()).unwrap_or("");
                    if let Ok(draft) = self.journal.get_draft(draft_id) {
                        if draft.status == DraftStatus::Approved {
                            let tally = self.tally.clone();
                            let xml = build_voucher_import_xml(&draft);

                            // Execute in worker thread with fresh runtime
                            let (tx, rx) = std::sync::mpsc::channel();
                            std::thread::spawn(move || {
                                if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                                    let res = rt.block_on(async move { tally.post_voucher_xml(&xml).await });
                                    let _ = tx.send(res);
                                }
                            });

                            match rx.recv_timeout(Duration::from_secs(5)) {
                                Ok(Ok(im)) => {
                                    if im.created > 0 {
                                        let _ = self.journal.record_commit_result(&draft, DraftStatus::Committed, "Posted via Companion UI");
                                        outcome = json!({ "success": true, "outcome": "Voucher created in Tally" });
                                    } else {
                                        outcome = json!({ "success": false, "error": im.error_message.unwrap_or_else(|| "Tally rejected voucher".to_string()) });
                                    }
                                }
                                Ok(Err(e)) => {
                                    outcome = json!({ "success": false, "error": e.to_string() });
                                }
                                Err(_) => {
                                    outcome = json!({ "success": false, "error": "Tally request timed out" });
                                }
                            }
                        } else {
                            outcome = json!({ "success": false, "error": "Draft is not in Approved state" });
                        }
                    }
                }

                let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url == "/api/extract" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "error": "Invalid extraction request payload" });
                let mut status_code = 400;

                if let Ok(val) = parsed {
                    let name = val.get("name").and_then(|n| n.as_str()).unwrap_or("document.json").to_string();
                    let mime = val.get("mime").and_then(|m| m.as_str()).unwrap_or("application/json").to_string();
                    let content_str = val.get("content").and_then(|c| c.as_str()).unwrap_or("");

                    let doc = crate::byok::Document {
                        mime,
                        bytes: content_str.as_bytes().to_vec(),
                        name,
                    };

                    match crate::byok::parse_invoice(&doc) {
                        Ok(extraction) => {
                            outcome = serde_json::to_value(extraction).unwrap_or_default();
                            status_code = 200;
                        }
                        Err(err) => {
                            outcome = json!({ "error": err.to_string() });
                        }
                    }
                }

                let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header).with_status_code(status_code);
                let _ = request.respond(resp);
                continue;
            }

            // 404 fallback
            let resp = Response::from_string("Not Found").with_status_code(404);
            let _ = request.respond(resp);
        }

        Ok(())
    }
}
