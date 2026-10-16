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
const GUIDE_CLAUDE_HTML: &str = include_str!("assets/guide-claude.html");
const GUIDE_CHATGPT_HTML: &str = include_str!("assets/guide-chatgpt.html");
const GUIDE_GEMINI_HTML: &str = include_str!("assets/guide-gemini.html");
const GUIDE_CURSOR_HTML: &str = include_str!("assets/guide-cursor.html");
const GUIDE_WINDSURF_HTML: &str = include_str!("assets/guide-windsurf.html");
const GUIDE_CLINE_HTML: &str = include_str!("assets/guide-cline.html");
const GUIDES_HUB_HTML: &str = include_str!("assets/guides.html");

pub struct CompanionUiServer {
    config: Arc<std::sync::RwLock<Config>>,
    tally: Arc<std::sync::RwLock<Arc<TallyClient>>>,
    journal: Arc<JournalManager>,
    audit: Arc<AuditLogger>,
    launch_token: String,
    mcp_server: Arc<std::sync::RwLock<crate::mcp::McpServer>>,
}

impl CompanionUiServer {
    pub fn new(config: Config) -> Self {
        let tally = Arc::new(std::sync::RwLock::new(Arc::new(TallyClient::new(
            config.tally.clone(),
        ))));
        let mcp_server = Arc::new(std::sync::RwLock::new(crate::mcp::McpServer::new(
            config.clone(),
        )));
        let config = Arc::new(std::sync::RwLock::new(config));
        let journal =
            Arc::new(JournalManager::new().expect("Failed to initialize journal manager"));
        let audit = Arc::new(AuditLogger::new());
        let launch_token = format!("tok_{}", uuid::Uuid::new_v4().simple());

        Self {
            config,
            tally,
            journal,
            audit,
            launch_token,
            mcp_server,
        }
    }

    #[allow(dead_code)]
    pub fn launch_token(&self) -> &str {
        &self.launch_token
    }

    pub fn run(&self, port: u16) -> Result<(), Box<dyn std::error::Error>> {
        let mut server = None;
        let mut bound_port = port;

        // Try binding to the preferred port, or fall back to the next available port in range
        for p in port..=(port + 20) {
            let addr = format!("127.0.0.1:{}", p);
            match Server::http(&addr) {
                Ok(srv) => {
                    server = Some(srv);
                    bound_port = p;
                    break;
                }
                Err(e) => {
                    tracing::debug!("Port {} unavailable: {}", p, e);
                }
            }
        }

        let server = server
            .ok_or_else(|| format!("Failed to bind to any port in range {}-{}", port, port + 20))?;

        let url = format!(
            "http://127.0.0.1:{}/?token={}",
            bound_port, self.launch_token
        );
        println!("=== TallyMCP Companion UI Running ===");
        println!("URL: {}", url);
        println!("Press Ctrl+C to terminate the companion UI.");

        // Automatically open the Companion UI URL in the default browser unless disabled
        if std::env::var("TALLY_NO_OPEN").unwrap_or_default() != "1" {
            if let Err(e) = open::that_detached(&url) {
                eprintln!("Note: Could not automatically open browser: {}", e);
                eprintln!("Please open the URL manually in your browser: {}", url);
            }
        }

        // Fast sync reachability check helper supporting both localhost and LAN IPs
        let check_tally_quick = |host: &str, port: u16| -> (bool, u128) {
            use std::net::{TcpStream, ToSocketAddrs};
            let start = std::time::Instant::now();
            let addr = format!("{}:{}", host, port);
            if let Ok(mut addrs) = addr.to_socket_addrs() {
                if let Some(sock_addr) = addrs.next() {
                    let reachable =
                        TcpStream::connect_timeout(&sock_addr, Duration::from_millis(800)).is_ok();
                    let elapsed = start.elapsed().as_millis();
                    return (reachable, elapsed);
                }
            }
            (false, start.elapsed().as_millis())
        };

        // Helper to query company list from Tally with timeout
        let query_companies = |tally_client: Arc<TallyClient>| -> Vec<String> {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    let res = rt.block_on(async move { tally_client.list_companies().await });
                    let _ = tx.send(res);
                }
            });
            rx.recv_timeout(Duration::from_millis(1500))
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default()
        };

        for mut request in server.incoming_requests() {
            let raw_url = request.url().to_string();
            let url_path = raw_url.split('?').next().unwrap_or("/").to_string();
            let method = request.method().as_str().to_string();

            // Strict Host header verification against DNS rebinding
            let host_header = request
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("host"));
            if let Some(h) = host_header {
                let h_val = h.value.as_str();
                if !h_val.starts_with("127.0.0.1") && !h_val.starts_with("localhost") {
                    let resp = Response::from_string("Forbidden: Host header not loopback")
                        .with_status_code(403);
                    let _ = request.respond(resp);
                    continue;
                }
            }

            // Strict Cross-Origin Request Forgery (CSRF) protection on state-modifying requests
            if method == "POST" {
                let origin_header = request
                    .headers()
                    .iter()
                    .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("origin"));
                if let Some(orig) = origin_header {
                    let o_val = orig.value.as_str();
                    if !o_val.starts_with("http://127.0.0.1")
                        && !o_val.starts_with("http://localhost")
                    {
                        let resp =
                            Response::from_string("Forbidden: Cross-origin request rejected")
                                .with_status_code(403);
                        let _ = request.respond(resp);
                        continue;
                    }
                }
            }

            let guide_content = match url_path.as_str() {
                "/" | "/index.html" => Some(UI_HTML),
                "/guide-claude.html" | "/guide/claude" => Some(GUIDE_CLAUDE_HTML),
                "/guide-chatgpt.html" | "/guide/chatgpt" => Some(GUIDE_CHATGPT_HTML),
                "/guide-gemini.html" | "/guide/gemini" => Some(GUIDE_GEMINI_HTML),
                "/guide-cursor.html" | "/guide/cursor" => Some(GUIDE_CURSOR_HTML),
                "/guide-windsurf.html" | "/guide/windsurf" => Some(GUIDE_WINDSURF_HTML),
                "/guide-cline.html" | "/guide/cline" => Some(GUIDE_CLINE_HTML),
                "/guides.html" | "/guides" | "/guide.html" | "/guide" => Some(GUIDES_HUB_HTML),
                _ => None,
            };

            if let Some(content) = guide_content {
                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                        .unwrap();
                let csp = Header::from_bytes(
                    &b"Content-Security-Policy"[..],
                    &b"default-src 'self' 'unsafe-inline' data:"[..],
                )
                .unwrap();
                let resp = Response::from_string(content)
                    .with_header(header)
                    .with_header(csp);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/status" && method == "GET" {
                let cfg = self.config.read().unwrap().clone();
                let tally_arc = self.tally.read().unwrap().clone();
                let (reachable, latency_ms) = check_tally_quick(&cfg.tally.host, cfg.tally.port);

                let loaded_companies = if reachable {
                    query_companies(tally_arc.clone())
                } else {
                    Vec::new()
                };

                let payload = json!({
                    "reachable": reachable,
                    "target_url": tally_arc.target_url(),
                    "host": cfg.tally.host,
                    "port": cfg.tally.port,
                    "circuit_breaker": "Closed",
                    "write_mode": cfg.safety.write_mode,
                    "loaded_companies": loaded_companies,
                    "active_company": loaded_companies.first().cloned(),
                    "latency_ms": latency_ms,
                    "last_sync": chrono::Utc::now().to_rfc3339()
                });

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(payload.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // Change Tally URL (Host / Port)
            if url_path == "/api/tally/config" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request body" });
                let mut status_code = 400;

                if let Ok(val) = parsed {
                    let new_host = val
                        .get("host")
                        .and_then(|h| h.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let new_port = val
                        .get("port")
                        .and_then(|p| p.as_u64())
                        .map(|p| p as u16)
                        .unwrap_or(0);

                    if new_host.is_empty() {
                        outcome = json!({ "success": false, "error": "Host / IP address cannot be empty." });
                    } else if new_port == 0 {
                        outcome = json!({ "success": false, "error": "Port number must be between 1 and 65535." });
                    } else if let Err(e) =
                        crate::tally::client::validate_tally_target_host(&new_host)
                    {
                        outcome = json!({ "success": false, "error": e.to_string() });
                    } else {
                        // Validated! Update config in memory and save to disk
                        {
                            let mut cfg = self.config.write().unwrap();
                            cfg.tally.host = new_host.clone();
                            cfg.tally.port = new_port;
                            cfg.tally.allow_remote_tally =
                                new_host != "127.0.0.1" && new_host != "localhost";
                            let _ = crate::config::save_config(&cfg);
                        }

                        // Reconfigure TallyClient and McpServer
                        let (new_tally, cfg_clone) = {
                            let cfg = self.config.read().unwrap();
                            (Arc::new(TallyClient::new(cfg.tally.clone())), cfg.clone())
                        };
                        {
                            let mut t_lock = self.tally.write().unwrap();
                            *t_lock = new_tally.clone();
                        }
                        *self.mcp_server.write().unwrap() = crate::mcp::McpServer::new(cfg_clone);

                        self.audit.log(
                            "companion_ui_change_target",
                            None,
                            None,
                            &format!("Target changed to {}:{}", new_host, new_port),
                        );

                        // Test reachability immediately
                        let (reachable, latency_ms) = check_tally_quick(&new_host, new_port);
                        let loaded_companies = if reachable {
                            query_companies(new_tally.clone())
                        } else {
                            Vec::new()
                        };

                        outcome = json!({
                            "success": true,
                            "reachable": reachable,
                            "target_url": format!("http://{}:{}", new_host, new_port),
                            "host": new_host,
                            "port": new_port,
                            "latency_ms": latency_ms,
                            "loaded_companies": loaded_companies,
                            "active_company": loaded_companies.first().cloned()
                        });
                        status_code = 200;
                    }
                }

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string())
                    .with_header(header)
                    .with_status_code(status_code);
                let _ = request.respond(resp);
                continue;
            }

            // Reconnect button handler
            if url_path == "/api/reconnect" && method == "POST" {
                let cfg = self.config.read().unwrap().clone();
                let tally_arc = self.tally.read().unwrap().clone();

                let (reachable, latency_ms) = check_tally_quick(&cfg.tally.host, cfg.tally.port);
                let loaded_companies = if reachable {
                    query_companies(tally_arc.clone())
                } else {
                    Vec::new()
                };

                let message = if reachable {
                    format!(
                        "Successfully connected to TallyPrime at {}:{} ({}ms). Open companies: {}",
                        cfg.tally.host,
                        cfg.tally.port,
                        latency_ms,
                        if loaded_companies.is_empty() {
                            "None (Open a company in Tally)".to_string()
                        } else {
                            loaded_companies.join(", ")
                        }
                    )
                } else {
                    format!("Cannot reach TallyPrime at {}:{}. Ensure Tally is open and ODBC/XML port {} is enabled in F1 ▸ Settings ▸ Connectivity.",
                        cfg.tally.host, cfg.tally.port, cfg.tally.port
                    )
                };

                let outcome = json!({
                    "success": reachable,
                    "reachable": reachable,
                    "target_url": format!("http://{}:{}", cfg.tally.host, cfg.tally.port),
                    "host": cfg.tally.host,
                    "port": cfg.tally.port,
                    "latency_ms": latency_ms,
                    "loaded_companies": loaded_companies,
                    "active_company": loaded_companies.first().cloned(),
                    "message": message
                });

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // Sync button handler
            if url_path == "/api/sync" && method == "POST" {
                let cfg = self.config.read().unwrap().clone();
                let tally_arc = self.tally.read().unwrap().clone();

                let (reachable, latency_ms) = check_tally_quick(&cfg.tally.host, cfg.tally.port);
                let loaded_companies = if reachable {
                    query_companies(tally_arc.clone())
                } else {
                    Vec::new()
                };
                let drafts = self.journal.list_pending_drafts().unwrap_or_default();

                let outcome = json!({
                    "success": true,
                    "reachable": reachable,
                    "target_url": format!("http://{}:{}", cfg.tally.host, cfg.tally.port),
                    "host": cfg.tally.host,
                    "port": cfg.tally.port,
                    "latency_ms": latency_ms,
                    "loaded_companies": loaded_companies,
                    "active_company": loaded_companies.first().cloned(),
                    "drafts_count": drafts.len(),
                    "synced_at": chrono::Utc::now().to_rfc3339()
                });

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // Get BYOK AI Configuration
            if url_path == "/api/byok/config" && method == "GET" {
                let cfg = self.config.read().unwrap().clone();
                let provider = if cfg.ai.provider.is_empty() {
                    "anthropic".to_string()
                } else {
                    cfg.ai.provider.clone()
                };
                let masked_key = crate::byok::get_masked_api_key(&provider).unwrap_or_default();
                let has_api_key = crate::byok::get_api_key(&provider).is_some();

                let outcome = json!({
                    "provider": provider,
                    "profile": cfg.ai.profile,
                    "model": cfg.ai.model,
                    "base_url": cfg.ai.base_url,
                    "has_api_key": has_api_key,
                    "masked_api_key": masked_key,
                    "builtin_hosts": crate::byok::BUILTIN_ALLOWED_HOSTS,
                    "extra_allowed_hosts": cfg.ai.extra_allowed_hosts
                });

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // Save BYOK AI Configuration & API Key
            if url_path == "/api/byok/config" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request body" });
                let mut status_code = 400;

                if let Ok(val) = parsed {
                    let provider = val
                        .get("provider")
                        .and_then(|p| p.as_str())
                        .unwrap_or("anthropic")
                        .trim()
                        .to_lowercase();
                    let api_key = val
                        .get("api_key")
                        .and_then(|k| k.as_str())
                        .unwrap_or("")
                        .trim();
                    let model = val
                        .get("model")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .trim();
                    let base_url = val
                        .get("base_url")
                        .and_then(|b| b.as_str())
                        .unwrap_or("")
                        .trim();
                    let profile = val
                        .get("profile")
                        .and_then(|p| p.as_str())
                        .unwrap_or("cloud")
                        .trim();

                    if !api_key.is_empty() {
                        let _ = crate::byok::set_api_key(&provider, api_key);
                    }

                    {
                        let mut cfg = self.config.write().unwrap();
                        cfg.ai.provider = provider.clone();
                        cfg.ai.profile = profile.to_string();
                        cfg.ai.model = model.to_string();
                        cfg.ai.base_url = base_url.to_string();
                        let _ = crate::config::save_config(&cfg);
                        let cfg_clone = cfg.clone();
                        *self.mcp_server.write().unwrap() = crate::mcp::McpServer::new(cfg_clone);
                    }

                    self.audit.log(
                        "companion_ui_change_ai",
                        None,
                        None,
                        &format!(
                            "AI settings saved: provider={}, profile={}",
                            provider, profile
                        ),
                    );

                    let masked_key = crate::byok::get_masked_api_key(&provider).unwrap_or_default();
                    outcome = json!({
                        "success": true,
                        "provider": provider,
                        "profile": profile,
                        "model": model,
                        "base_url": base_url,
                        "has_api_key": crate::byok::get_api_key(&provider).is_some(),
                        "masked_api_key": masked_key
                    });
                    status_code = 200;
                }

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string())
                    .with_header(header)
                    .with_status_code(status_code);
                let _ = request.respond(resp);
                continue;
            }

            // Test BYOK / AI provider connection
            if url_path == "/api/byok/test" && method == "POST" {
                let cfg = self.config.read().unwrap().clone();
                let provider = cfg.ai.provider.trim().to_lowercase();

                let outcome = if provider == "ollama" {
                    let base_url = if cfg.ai.base_url.trim().is_empty() {
                        "http://127.0.0.1:11434"
                    } else {
                        cfg.ai.base_url.trim()
                    };
                    let clean = base_url
                        .trim_start_matches("http://")
                        .trim_start_matches("https://");
                    let host_port = clean.split('/').next().unwrap_or(clean);
                    let hp_parts: Vec<&str> = host_port.split(':').collect();
                    let host = hp_parts.first().unwrap_or(&"127.0.0.1");
                    let port = hp_parts
                        .get(1)
                        .and_then(|p| p.parse::<u16>().ok())
                        .unwrap_or(11434);

                    let (reachable, ms) = check_tally_quick(host, port);
                    if reachable {
                        json!({ "success": true, "message": format!("Local Ollama server is online and reachable at {} ({}ms).", base_url, ms) })
                    } else {
                        json!({ "success": false, "message": format!("Cannot connect to Ollama at {}. Make sure 'ollama serve' is running.", base_url) })
                    }
                } else {
                    let has_key = crate::byok::get_api_key(&provider).is_some();
                    if has_key {
                        json!({ "success": true, "message": format!("API key for provider '{}' is configured and stored securely.", provider) })
                    } else {
                        json!({ "success": false, "message": format!("No API key found for '{}'. Please enter and save your API key above.", provider) })
                    }
                };

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // Direct MCP Protocol Endpoint (for Claude Custom Connectors & HTTP MCP clients)
            if url_path == "/mcp" {
                let cors_origin =
                    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
                let cors_methods = Header::from_bytes(
                    &b"Access-Control-Allow-Methods"[..],
                    &b"POST, GET, OPTIONS"[..],
                )
                .unwrap();
                let cors_headers = Header::from_bytes(
                    &b"Access-Control-Allow-Headers"[..],
                    &b"Content-Type, Authorization, x-api-key"[..],
                )
                .unwrap();

                if method == "OPTIONS" {
                    let resp = Response::empty(200)
                        .with_header(cors_origin)
                        .with_header(cors_methods)
                        .with_header(cors_headers);
                    let _ = request.respond(resp);
                    continue;
                }

                if method == "GET" {
                    let info = json!({
                        "status": "online",
                        "server": "tally-mcp",
                        "version": env!("CARGO_PKG_VERSION"),
                        "protocol": "mcp",
                        "endpoint": "/mcp",
                        "capabilities": { "tools": { "listChanged": false } }
                    });
                    let header =
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                    let resp = Response::from_string(info.to_string())
                        .with_header(header)
                        .with_header(cors_origin);
                    let _ = request.respond(resp);
                    continue;
                }

                if method == "POST" {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);

                    let mcp = self.mcp_server.read().unwrap().clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            let res: serde_json::Value = rt.block_on(async move {
                                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                                    if let Some(id) = parsed.get("id").cloned() {
                                        let m = parsed.get("method").and_then(|m| m.as_str()).unwrap_or("");
                                        let p = parsed.get("params").cloned().unwrap_or(serde_json::Value::Null);
                                        return mcp.handle_request(m, p, id).await;
                                    } else {
                                        let m = parsed.get("method").and_then(|m| m.as_str()).unwrap_or("");
                                        if m == "notifications/initialized" {
                                            return json!({ "status": "initialized" });
                                        }
                                    }
                                }
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": serde_json::Value::Null,
                                    "error": { "code": -32700, "message": "Parse error / Invalid JSON-RPC request" }
                                })
                            });
                            let _ = tx.send(res);
                        }
                    });

                    let outcome = rx
                        .recv_timeout(Duration::from_secs(30))
                        .unwrap_or_else(|_| {
                            json!({
                                "jsonrpc": "2.0",
                                "id": serde_json::Value::Null,
                                "error": { "code": -32603, "message": "MCP request timed out" }
                            })
                        });

                    let header =
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                    let resp = Response::from_string(outcome.to_string())
                        .with_header(header)
                        .with_header(cors_origin);
                    let _ = request.respond(resp);
                    continue;
                }
            }

            // Ready-to-copy MCP Client Configs
            if url_path == "/api/mcp/configs" && method == "GET" {
                let cfg = self.config.read().unwrap().clone();
                let host = cfg.tally.host.clone();
                let port = cfg.tally.port;

                let serve_args = if host != "127.0.0.1" && host != "localhost" {
                    vec![
                        "serve".to_string(),
                        "--host".to_string(),
                        host.clone(),
                        "--port".to_string(),
                        port.to_string(),
                    ]
                } else {
                    vec!["serve".to_string()]
                };

                let current_exe_cmd = std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        let s = p.to_string_lossy().to_string();
                        if s.contains("target") && (s.contains("deps") || s.contains("debug")) {
                            None
                        } else {
                            Some(s)
                        }
                    })
                    .unwrap_or_else(|| "tally-mcp".to_string());

                let claude_diag = crate::config::diagnose_client_config("claude");

                let claude_config = json!({
                    "mcpServers": {
                        "tally": {
                            "command": current_exe_cmd,
                            "args": serve_args
                        }
                    }
                });

                let cursor_config = json!({
                    "mcpServers": {
                        "tally": {
                            "command": "tally-mcp",
                            "args": serve_args
                        }
                    }
                });

                let windsurf_config = json!({
                    "mcpServers": {
                        "tally": {
                            "command": "tally-mcp",
                            "args": serve_args
                        }
                    }
                });

                let cline_config = json!({
                    "mcpServers": {
                        "tally": {
                            "command": "tally-mcp",
                            "args": serve_args,
                            "disabled": false,
                            "autoApprove": []
                        }
                    }
                });

                let payload = json!({
                    "current_host": host,
                    "current_port": port,
                    "claude_custom_connector": {
                        "name": "TallyMCP",
                        "url": format!("http://127.0.0.1:{}/mcp", bound_port),
                        "step": "Step 1 of 2",
                        "heading": "Add custom connector",
                        "subheading": "Connect Claude to your data and tools. Learn more about connectors or get started with pre-built ones.",
                        "name_label": "Name",
                        "name_hint": "Shown in the connectors list.",
                        "url_label": "MCP server URL",
                        "url_hint": format!("The address where the server accepts MCP requests (http://127.0.0.1:{}/mcp)", bound_port),
                        "trust_notice": "Only use connectors from developers you trust. Anthropic does not control which tools developers make available and cannot verify that they will work as intended or that they won't change."
                    },
                    "claude": {
                        "filename": "claude_desktop_config.json",
                        "paths": {
                            "windows": "%APPDATA%\\Claude\\claude_desktop_config.json",
                            "macos": "~/Library/Application Support/Claude/claude_desktop_config.json",
                            "linux": "~/.config/Claude/claude_desktop_config.json"
                        },
                        "json": serde_json::to_string_pretty(&claude_config).unwrap_or_default()
                    },
                    "cursor": {
                        "filename": ".cursor/mcp.json",
                        "paths": {
                            "project": ".cursor/mcp.json",
                            "windows": "%APPDATA%\\Cursor\\User\\globalStorage\\mcp.json",
                            "macos": "~/Library/Application Support/Cursor/User/globalStorage/mcp.json",
                            "linux": "~/.config/Cursor/User/globalStorage/mcp.json"
                        },
                        "json": serde_json::to_string_pretty(&cursor_config).unwrap_or_default()
                    },
                    "windsurf": {
                        "filename": "mcp_config.json",
                        "paths": {
                            "all": "~/.codeium/windsurf/mcp_config.json"
                        },
                        "json": serde_json::to_string_pretty(&windsurf_config).unwrap_or_default()
                    },
                    "cline": {
                        "filename": "cline_mcp_settings.json",
                        "paths": {
                            "windows": "%APPDATA%\\Code\\User\\globalStorage\\saoudrizwan.claude-dev\\settings\\cline_mcp_settings.json",
                            "macos": "~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
                            "linux": "~/.config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"
                        },
                        "json": serde_json::to_string_pretty(&cline_config).unwrap_or_default()
                    },
                    "chatgpt": {
                        "filename": "ChatGPT Desktop (Settings > Integration > Plugins)",
                        "paths": {
                            "all": "ChatGPT Desktop ▸ Edit ▸ Settings ▸ Integration ▸ Plugins ▸ Add MCP Server"
                        },
                        "json": serde_json::to_string_pretty(&json!({
                            "name": "tally",
                            "type": "stdio",
                            "command": "tally-mcp",
                            "args": serve_args
                        })).unwrap_or_default()
                    },
                    "gemini": {
                        "filename": "gemini_mcp_config.json",
                        "paths": {
                            "all": "~/.gemini/mcp_config.json"
                        },
                        "json": serde_json::to_string_pretty(&json!({
                            "mcpServers": {
                                "tally": {
                                    "command": "tally-mcp",
                                    "args": serve_args
                                }
                            }
                        })).unwrap_or_default()
                    },
                    "claude_diagnostic": {
                        "is_healthy": claude_diag.is_healthy,
                        "tally_configured": claude_diag.tally_configured,
                        "has_serve_arg": claude_diag.has_serve_arg,
                        "command_exists": claude_diag.command_exists,
                        "message": claude_diag.message,
                        "config_path": claude_diag.config_path.map(|p| p.to_string_lossy().to_string())
                    },
                    "cli": {
                        "command": format!("tally-mcp serve{}", if host != "127.0.0.1" && host != "localhost" { format!(" --host {} --port {}", host, port) } else { String::new() })
                    }
                });

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(payload.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // 1-Click Auto-Configuration for Claude Desktop & other MCP clients
            if url_path == "/api/mcp/auto-configure" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let req_payload: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
                let client = req_payload["client"].as_str().unwrap_or("claude");

                let cfg = self.config.read().unwrap().clone();
                let host = cfg.tally.host.clone();
                let port = cfg.tally.port;

                let outcome = match crate::config::auto_configure_client(client, &host, port) {
                    Ok((path, exe)) => json!({
                        "success": true,
                        "client": client,
                        "config_path": path.to_string_lossy().to_string(),
                        "command": exe,
                        "message": format!("Successfully configured {}! Please restart the client to apply changes.", client.to_uppercase())
                    }),
                    Err(e) => json!({
                        "success": false,
                        "client": client,
                        "error": e.clone(),
                        "message": format!("Failed to auto-configure {}: {}", client, e)
                    }),
                };

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            // Export .mcpb bundle for one-click installation in Claude Desktop
            if url_path == "/api/mcp/export-mcpb" && method == "GET" {
                let cfg = self.config.read().unwrap().clone();
                let host = cfg.tally.host.clone();
                let port = cfg.tally.port;

                let serve_args = if host != "127.0.0.1" && host != "localhost" {
                    vec![
                        "serve".to_string(),
                        "--host".to_string(),
                        host.clone(),
                        "--port".to_string(),
                        port.to_string(),
                    ]
                } else {
                    vec!["serve".to_string()]
                };

                let mcpb_manifest = json!({
                    "dxt_version": "0.1",
                    "name": "tally-mcp",
                    "display_name": "TallyMCP",
                    "version": "1.0.0",
                    "description": "Open Source Model Context Protocol server connecting TallyPrime accounting software to AI assistants.",
                    "author": {
                        "name": "Shivvam",
                        "url": "https://github.com/Shivvam/tallyprimemcp"
                    },
                    "homepage": "https://tallymcp.vercel.app/",
                    "repository": "https://github.com/Shivvam/tallyprimemcp",
                    "server": {
                        "type": "binary",
                        "mcp_config": {
                            "command": "tally-mcp",
                            "args": serve_args,
                            "env": {
                                "TALLY_HOST": host,
                                "TALLY_PORT": port.to_string()
                            }
                        }
                    },
                    "tools": [
                        "tally_list_companies",
                        "tally_set_active_company",
                        "tally_list_ledgers",
                        "tally_get_ledger",
                        "tally_trial_balance",
                        "tally_day_book",
                        "tally_draft_voucher",
                        "tally_preview_draft"
                    ]
                });

                let manifest_bytes = serde_json::to_vec_pretty(&mcpb_manifest).unwrap_or_default();
                let zip_bytes = create_stored_zip("manifest.json", &manifest_bytes);

                let content_type = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/vnd.claude.extension"[..],
                )
                .unwrap();
                let content_disp = Header::from_bytes(
                    &b"Content-Disposition"[..],
                    &b"attachment; filename=\"tally-mcp.mcpb\""[..],
                )
                .unwrap();
                let resp = Response::from_data(zip_bytes)
                    .with_header(content_type)
                    .with_header(content_disp);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/drafts" && method == "GET" {
                let drafts = self.journal.list_pending_drafts().unwrap_or_default();
                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp =
                    Response::from_string(serde_json::to_string(&drafts).unwrap_or_default())
                        .with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/drafts/approve" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request" });

                if let Ok(val) = parsed {
                    let draft_id = val.get("draft_id").and_then(|d| d.as_str()).unwrap_or("");
                    let content_hash = val
                        .get("content_hash")
                        .and_then(|h| h.as_str())
                        .unwrap_or("");

                    if let Ok(mut draft) = self.journal.get_draft(draft_id) {
                        if draft.content_hash == content_hash {
                            draft.status = DraftStatus::Approved;
                            if self.journal.save_draft(&draft).is_ok() {
                                self.audit.log(
                                    "companion_ui_approve",
                                    Some(draft_id),
                                    Some(&draft.company),
                                    "human_approved",
                                );
                                outcome = json!({ "success": true, "draft_id": draft_id });
                            }
                        } else {
                            outcome = json!({ "success": false, "error": "Content hash mismatch. Draft was altered." });
                        }
                    }
                }

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/drafts/commit" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request" });

                if let Ok(val) = parsed {
                    let draft_id = val.get("draft_id").and_then(|d| d.as_str()).unwrap_or("");
                    if let Ok(draft) = self.journal.get_draft(draft_id) {
                        if draft.status == DraftStatus::Approved {
                            let tally = self.tally.read().unwrap().clone();
                            let xml = build_voucher_import_xml(&draft);

                            // Execute in worker thread with fresh runtime
                            let (tx, rx) = std::sync::mpsc::channel();
                            std::thread::spawn(move || {
                                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                {
                                    let res =
                                        rt.block_on(
                                            async move { tally.post_voucher_xml(&xml).await },
                                        );
                                    let _ = tx.send(res);
                                }
                            });

                            match rx.recv_timeout(Duration::from_secs(5)) {
                                Ok(Ok(im)) => {
                                    if im.created > 0 {
                                        let _ = self.journal.record_commit_result(
                                            &draft,
                                            DraftStatus::Committed,
                                            "Posted via Companion UI",
                                        );
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

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string()).with_header(header);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/extract" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "error": "Invalid extraction request payload" });
                let mut status_code = 400;

                if let Ok(val) = parsed {
                    let name = val
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("document.json")
                        .to_string();
                    let mime = val
                        .get("mime")
                        .and_then(|m| m.as_str())
                        .unwrap_or("application/json")
                        .to_string();
                    let content_b64 = val.get("content_base64").and_then(|c| c.as_str());
                    let content_str = val.get("content").and_then(|c| c.as_str()).unwrap_or("");

                    use base64::Engine;
                    let bytes = if let Some(b64) = content_b64 {
                        base64::engine::general_purpose::STANDARD
                            .decode(b64.trim())
                            .unwrap_or_else(|_| content_str.as_bytes().to_vec())
                    } else {
                        content_str.as_bytes().to_vec()
                    };

                    let doc = crate::byok::Document { mime, bytes, name };

                    let cfg = self.config.read().unwrap().clone();
                    match crate::byok::extract_document(&cfg, &doc) {
                        Ok(extraction) => {
                            outcome = serde_json::to_value(extraction).unwrap_or_default();
                            status_code = 200;
                        }
                        Err(err) => {
                            outcome = json!({ "error": err.to_string() });
                        }
                    }
                }

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string())
                    .with_header(header)
                    .with_status_code(status_code);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/chat" && method == "POST" {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
                let mut outcome = json!({ "success": false, "error": "Invalid request payload" });
                let mut status_code = 400;

                if let Ok(val) = parsed {
                    let message = val
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if message.is_empty() {
                        outcome = json!({ "success": false, "error": "Message cannot be empty" });
                    } else {
                        let cfg = self.config.read().unwrap().clone();
                        let provider = if cfg.ai.provider.is_empty() {
                            "anthropic".to_string()
                        } else {
                            cfg.ai.provider.clone()
                        };

                        // Check if BYOK is configured
                        let has_key =
                            provider == "ollama" || crate::byok::get_api_key(&provider).is_some();
                        if !has_key {
                            outcome = json!({
                                "success": false,
                                "error": "BYOK_REQUIRED",
                                "message": format!("API Key for provider '{}' is not configured. Please enter your API key in BYOK Settings before chatting.", provider)
                            });
                        } else {
                            // Gather Tally context
                            let tally_arc = self.tally.read().unwrap().clone();
                            let (reachable, latency) =
                                check_tally_quick(&cfg.tally.host, cfg.tally.port);
                            let loaded_companies = if reachable {
                                query_companies(tally_arc.clone())
                            } else {
                                Vec::new()
                            };
                            let active_comp = loaded_companies
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "None".to_string());

                            let tally_ctx = format!(
                                "Tally Server: http://{}:{}\nReachable: {}\nLatency: {}ms\nActive Company: {}\nOpen Companies: {}",
                                cfg.tally.host, cfg.tally.port, reachable, latency, active_comp,
                                if loaded_companies.is_empty() { "None (no company open)".to_string() } else { loaded_companies.join(", ") }
                            );

                            let (tx, rx) = std::sync::mpsc::channel();
                            let msg_clone = message.clone();
                            let cfg_clone = cfg.clone();
                            std::thread::spawn(move || {
                                let res = crate::byok::chat(&cfg_clone, &msg_clone, &tally_ctx);
                                let _ = tx.send(res);
                            });

                            match rx.recv_timeout(Duration::from_secs(60)) {
                                Ok(Ok(reply)) => {
                                    outcome = json!({
                                        "success": true,
                                        "response": reply,
                                        "provider": provider,
                                        "model": cfg.ai.model,
                                        "active_company": active_comp,
                                        "tally_reachable": reachable
                                    });
                                    status_code = 200;
                                }
                                Ok(Err(e)) => {
                                    outcome = json!({
                                        "success": false,
                                        "error": e.to_string(),
                                        "message": format!("AI Provider error: {}", e)
                                    });
                                    status_code = 502;
                                }
                                Err(_) => {
                                    outcome = json!({
                                        "success": false,
                                        "error": "Timeout",
                                        "message": "AI request timed out after 60 seconds."
                                    });
                                    status_code = 504;
                                }
                            }
                        }
                    }
                }

                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp = Response::from_string(outcome.to_string())
                    .with_header(header)
                    .with_status_code(status_code);
                let _ = request.respond(resp);
                continue;
            }

            if url_path == "/api/audit" && method == "GET" {
                let entries = self.audit.read_recent_entries(50);
                let header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let resp =
                    Response::from_string(serde_json::to_string(&entries).unwrap_or_default())
                        .with_header(header);
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

/// Creates an uncompressed ZIP archive containing a single file in memory.
/// This complies with standard PKZip specification without requiring extra crate dependencies.
pub fn create_stored_zip(filename: &str, data: &[u8]) -> Vec<u8> {
    let name_bytes = filename.as_bytes();
    let name_len = name_bytes.len() as u16;
    let size = data.len() as u32;

    // Standard IEEE 802.3 CRC-32
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if (crc & 1) != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    let crc = !crc;

    let mut zip = Vec::with_capacity(data.len() + 256);

    // 1. Local File Header (30 bytes + name_len + data.len())
    zip.extend_from_slice(b"PK\x03\x04");
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // method: 0 = stored
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod time
    zip.extend_from_slice(&0x5821u16.to_le_bytes()); // mod date: 2024-01-01
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes()); // compressed size
    zip.extend_from_slice(&size.to_le_bytes()); // uncompressed size
    zip.extend_from_slice(&name_len.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra field len
    zip.extend_from_slice(name_bytes);
    zip.extend_from_slice(data);

    let cd_offset = zip.len() as u32;

    // 2. Central Directory File Header (46 bytes + name_len)
    zip.extend_from_slice(b"PK\x01\x02");
    zip.extend_from_slice(&20u16.to_le_bytes()); // version made by: 2.0
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // method: 0 = stored
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod time
    zip.extend_from_slice(&0x5821u16.to_le_bytes()); // mod date: 2024-01-01
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes()); // compressed size
    zip.extend_from_slice(&size.to_le_bytes()); // uncompressed size
    zip.extend_from_slice(&name_len.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra field len
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment len
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk number start
    zip.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
    zip.extend_from_slice(&0u32.to_le_bytes()); // external attributes
    zip.extend_from_slice(&0u32.to_le_bytes()); // local header relative offset
    zip.extend_from_slice(name_bytes);

    let cd_len = (zip.len() as u32) - cd_offset;

    // 3. End of Central Directory Record (22 bytes)
    zip.extend_from_slice(b"PK\x05\x06");
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk number
    zip.extend_from_slice(&0u16.to_le_bytes()); // start disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // entries on this disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // total entries
    zip.extend_from_slice(&cd_len.to_le_bytes());
    zip.extend_from_slice(&cd_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment len

    zip
}
