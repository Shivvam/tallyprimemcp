use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub tally: TallyConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub toolsets: ToolsetsConfig,
    #[serde(default)]
    pub tokens: TokensConfig,
    #[serde(default)]
    pub audit: AuditConfig,
    #[serde(default)]
    pub schedule: ScheduleConfig,
    #[serde(default)]
    pub rules: RulesConfig,
    #[serde(default)]
    pub roles: RolesConfig,
    #[serde(default)]
    pub packs: PacksConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub cleanup: CleanupConfig,
    #[serde(default)]
    pub clients: Vec<ClientProfile>,
}

fn default_schema_version() -> u32 {
    2
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 2,
            tally: TallyConfig::default(),
            safety: SafetyConfig::default(),
            ai: AiConfig::default(),
            server: ServerConfig::default(),
            toolsets: ToolsetsConfig::default(),
            tokens: TokensConfig::default(),
            audit: AuditConfig::default(),
            schedule: ScheduleConfig::default(),
            rules: RulesConfig::default(),
            roles: RolesConfig::default(),
            packs: PacksConfig::default(),
            ui: UiConfig::default(),
            logging: LoggingConfig::default(),
            cleanup: CleanupConfig::default(),
            clients: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TallyConfig {
    pub host: String,
    pub port: u16,
    pub allow_remote_tally: bool,
    pub default_company: String,
    pub call_deadline_secs: u64,
    pub report_deadline_secs: u64,
    pub queue_depth: usize,
    pub breaker_failures: u32,
}

impl Default for TallyConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 9000,
            allow_remote_tally: false,
            default_company: String::new(),
            call_deadline_secs: 30,
            report_deadline_secs: 120,
            queue_depth: 8,
            breaker_failures: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub write_mode: String, // "off" | "approve" | "auto_under_limit"
    pub max_voucher_amount: f64,
    pub max_daily_total: f64,
    pub date_tolerance_days: u32,
    pub require_confirmation_below_confidence: f64,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            write_mode: "off".to_string(),
            max_voucher_amount: 100000.00,
            max_daily_total: 500000.00,
            date_tolerance_days: 7,
            require_confirmation_below_confidence: 0.90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub profile: String, // "cloud" | "offline"
    pub provider: String,
    pub model: String,
    #[serde(default = "default_model_fast")]
    pub model_fast: String,
    pub base_url: String,
    pub extra_allowed_hosts: Vec<String>,
    pub send_ledger_names_for_matching: bool,
}

fn default_model_fast() -> String {
    "claude-haiku-5-5".to_string()
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            profile: "cloud".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-5-5".to_string(),
            model_fast: default_model_fast(),
            base_url: String::new(),
            extra_allowed_hosts: Vec::new(),
            send_ledger_names_for_matching: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub http_enabled: bool,
    pub http_bind: String,
    pub http_port: u16,
    pub rate_limit_per_min: u32,
    pub allowed_origins: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_enabled: false,
            http_bind: "127.0.0.1".to_string(),
            http_port: 7410,
            rate_limit_per_min: 60,
            allowed_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsetsConfig {
    pub enabled: Vec<String>,
    pub max_rows_default: usize,
    pub detail_default: String,
}

impl Default for ToolsetsConfig {
    fn default() -> Self {
        Self {
            enabled: vec!["core".to_string()],
            max_rows_default: 25,
            detail_default: "summary".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokensConfig {
    pub max_tokens_hint: usize,
    pub budget_daily_soft: usize,
    pub ocr_max_pages: usize,
    pub ocr_max_edge_px: usize,
    pub cache_max_mb: usize,
}

impl Default for TokensConfig {
    fn default() -> Self {
        Self {
            max_tokens_hint: 1500,
            budget_daily_soft: 0,
            ocr_max_pages: 5,
            ocr_max_edge_px: 1568,
            cache_max_mb: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditWeightsConfig {
    pub inventory: f64,
    pub vouchers: f64,
    pub compliance: f64,
    pub bank: f64,
    pub related_party: f64,
    pub cost_centre: f64,
    pub books: f64,
}

impl Default for AuditWeightsConfig {
    fn default() -> Self {
        Self {
            inventory: 1.0,
            vouchers: 1.0,
            compliance: 1.0,
            bank: 0.8,
            related_party: 0.8,
            cost_centre: 0.6,
            books: 0.6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    pub keep_runs: usize,
    pub materiality_inr: f64,
    pub neg_timing_days: i64,
    pub phys_tol_pct: f64,
    pub phys_tol_inr: f64,
    pub expected_method: String,
    pub cc_tol_inr: f64,
    pub cc_drift_pct: f64,
    pub dup_days: i64,
    pub high_value_inr: f64,
    pub mad_k: f64,
    pub cash_pay_limit: f64,
    pub cash_pay_transport_limit: f64,
    pub cash_recv_limit: f64,
    pub cash_pay_exempt_ledgers: Vec<String>,
    pub transporter_ledgers: Vec<String>,
    pub weekly_off: Vec<String>,
    pub holidays: Vec<String>,
    pub work_hours: Vec<String>,
    pub related_parties: Vec<String>,
    pub msme_suppliers: Vec<String>,
    pub brs_stale_days: i64,
    pub match_tol_inr: f64,
    pub msme_days: i64,
    pub itc_unpaid_days: i64,
    pub benford_min_n: usize,
    pub yield_between_chunks_ms: u64,
    #[serde(default)]
    pub weights: AuditWeightsConfig,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            keep_runs: 20,
            materiality_inr: 100000.0,
            neg_timing_days: 15,
            phys_tol_pct: 1.0,
            phys_tol_inr: 5000.0,
            expected_method: "avg_cost".to_string(),
            cc_tol_inr: 1.0,
            cc_drift_pct: 30.0,
            dup_days: 3,
            high_value_inr: 500000.0,
            mad_k: 6.0,
            cash_pay_limit: 10000.0,
            cash_pay_transport_limit: 35000.0,
            cash_recv_limit: 200000.0,
            cash_pay_exempt_ledgers: Vec::new(),
            transporter_ledgers: Vec::new(),
            weekly_off: vec!["Sun".to_string()],
            holidays: Vec::new(),
            work_hours: vec!["09:00".to_string(), "20:00".to_string()],
            related_parties: Vec::new(),
            msme_suppliers: Vec::new(),
            brs_stale_days: 90,
            match_tol_inr: 1.0,
            msme_days: 45,
            itc_unpaid_days: 180,
            benford_min_n: 300,
            yield_between_chunks_ms: 50,
            weights: AuditWeightsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub audit: String, // "off" | "weekly" | "monthly"
    pub audit_day: String,
    pub audit_time: String,
    pub audit_areas: Vec<String>,
    pub audit_export: String,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            audit: "off".to_string(),
            audit_day: "Mon".to_string(),
            audit_time: "08:00".to_string(),
            audit_areas: vec!["all".to_string()],
            audit_export: "md".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesConfig {
    pub in_tax_file: String,
    pub checks_file: String,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            in_tax_file: "rules/in-tax-FY2026-27.json".to_string(),
            checks_file: "rules/audit-checks.json".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RolesConfig {
    pub enforce_maker_checker: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub company: String,
}

impl Default for ClientProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: "127.0.0.1".to_string(),
            port: 9000,
            company: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PacksConfig {
    pub enable_unsigned: bool,
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub auto_open_ui: bool,
    #[serde(default = "default_ui_port")]
    pub port: u16,
}

fn default_ui_port() -> u16 {
    9190
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            auto_open_ui: false,
            port: default_ui_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub retain_days: u32,
    pub max_log_mb: u64,
    pub max_audit_mb: u64,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            retain_days: 30,
            max_log_mb: 50,
            max_audit_mb: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    pub backup_before_cleanup: bool,
    pub backup_dir: String,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            backup_before_cleanup: true,
            backup_dir: String::new(),
        }
    }
}

pub fn get_app_data_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("TallyMCP")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("TallyMCP")
    } else {
        PathBuf::from(".tallymcp")
    }
}

pub fn get_local_app_data_dir() -> PathBuf {
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_appdata).join("TallyMCP")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("TallyMCP")
    } else {
        PathBuf::from(".tallymcp_local")
    }
}

pub fn get_client_data_dir(client: Option<&str>) -> PathBuf {
    let base = get_local_app_data_dir();
    if let Some(slug) = client {
        let clean = slug.trim().to_lowercase().replace(' ', "-");
        if !clean.is_empty() {
            return base.join("clients").join(clean);
        }
    }
    base
}

pub fn load_config() -> (Config, PathBuf) {
    let config_dir = get_app_data_dir();
    let config_path = config_dir.join("config.toml");

    let mut config = if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            toml::from_str::<Config>(&content).unwrap_or_default()
        } else {
            Config::default()
        }
    } else {
        Config::default()
    };

    // Environment variable overrides
    if let Ok(host) = std::env::var("TALLY_HOST").or_else(|_| std::env::var("TALLYMCP_TALLY_HOST"))
    {
        let trimmed = host.trim();
        if !trimmed.is_empty() {
            config.tally.host = trimmed.to_string();
            config.tally.allow_remote_tally = true;
        }
    }
    if let Ok(port_str) =
        std::env::var("TALLY_PORT").or_else(|_| std::env::var("TALLYMCP_TALLY_PORT"))
    {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            config.tally.port = port;
        }
    }
    if let Ok(mode) = std::env::var("TALLYMCP_SAFETY_WRITE_MODE") {
        config.safety.write_mode = mode.trim().to_string();
    }
    if let Ok(http_en) = std::env::var("TALLYMCP_SERVER_HTTP_ENABLED") {
        config.server.http_enabled = http_en.trim() == "true" || http_en.trim() == "1";
    }
    if let Ok(port_str) = std::env::var("TALLYMCP_SERVER_HTTP_PORT") {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            config.server.http_port = port;
        }
    }

    (config, config_path)
}

pub fn save_config(config: &Config) -> Result<PathBuf, std::io::Error> {
    let config_dir = get_app_data_dir();
    let config_path = config_dir.join("config.toml");

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let serialized =
        toml::to_string_pretty(config).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(&config_path, serialized)?;

    Ok(config_path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientDiagnostic {
    pub client: String,
    pub config_path: Option<PathBuf>,
    pub file_exists: bool,
    pub tally_configured: bool,
    pub command: Option<String>,
    pub command_exists: bool,
    pub has_serve_arg: bool,
    pub is_healthy: bool,
    pub message: String,
}

pub fn get_client_config_path(client: &str) -> Option<PathBuf> {
    match client.to_lowercase().as_str() {
        "claude" => {
            #[cfg(target_os = "windows")]
            {
                std::env::var("APPDATA").ok().map(|p| {
                    PathBuf::from(p)
                        .join("Claude")
                        .join("claude_desktop_config.json")
                })
            }
            #[cfg(target_os = "macos")]
            {
                std::env::var("HOME").ok().map(|p| {
                    PathBuf::from(p)
                        .join("Library")
                        .join("Application Support")
                        .join("Claude")
                        .join("claude_desktop_config.json")
                })
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                std::env::var("HOME").ok().map(|p| {
                    PathBuf::from(p)
                        .join(".config")
                        .join("Claude")
                        .join("claude_desktop_config.json")
                })
            }
        }
        "cursor" => {
            #[cfg(target_os = "windows")]
            {
                std::env::var("APPDATA").ok().map(|p| {
                    PathBuf::from(p)
                        .join("Cursor")
                        .join("User")
                        .join("globalStorage")
                        .join("mcp.json")
                })
            }
            #[cfg(target_os = "macos")]
            {
                std::env::var("HOME").ok().map(|p| {
                    PathBuf::from(p)
                        .join("Library")
                        .join("Application Support")
                        .join("Cursor")
                        .join("User")
                        .join("globalStorage")
                        .join("mcp.json")
                })
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                std::env::var("HOME").ok().map(|p| {
                    PathBuf::from(p)
                        .join(".config")
                        .join("Cursor")
                        .join("User")
                        .join("globalStorage")
                        .join("mcp.json")
                })
            }
        }
        "windsurf" => {
            let base = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()?;
            Some(
                PathBuf::from(base)
                    .join(".codeium")
                    .join("windsurf")
                    .join("mcp_config.json"),
            )
        }
        _ => None,
    }
}

pub fn resolve_executable_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        let s = exe.to_string_lossy().to_string();
        if !s.contains("target") || (!s.contains("deps") && !s.contains("debug")) {
            return s;
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            let msi_path = PathBuf::from(local_appdata)
                .join("Programs")
                .join("TallyMCP")
                .join("tally-mcp.exe");
            if msi_path.exists() {
                return msi_path.to_string_lossy().to_string();
            }
        }
    }
    "tally-mcp".to_string()
}

pub fn auto_configure_client(
    client: &str,
    host: &str,
    port: u16,
) -> Result<(PathBuf, String), String> {
    let path = get_client_config_path(client).ok_or_else(|| {
        format!(
            "Could not determine configuration directory for client '{}'",
            client
        )
    })?;

    let exe_path = resolve_executable_path();
    let serve_args = if host != "127.0.0.1" && host != "localhost" {
        vec![
            "serve".to_string(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ]
    } else {
        vec!["serve".to_string()]
    };

    let mut root: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "Failed to read existing config at {}: {}",
                path.display(),
                e
            )
        })?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !root.is_object() {
        root = serde_json::json!({});
    }

    let servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));

    if !servers.is_object() {
        *servers = serde_json::json!({});
    }

    servers.as_object_mut().unwrap().insert(
        "tallymcp.pro".to_string(),
        serde_json::json!({
            "command": exe_path.clone(),
            "args": serve_args
        }),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    let formatted =
        serde_json::to_string_pretty(&root).map_err(|e| format!("Serialization error: {}", e))?;
    std::fs::write(&path, formatted)
        .map_err(|e| format!("Failed to write configuration to {}: {}", path.display(), e))?;

    Ok((path, exe_path))
}

pub fn diagnose_client_config(client: &str) -> ClientDiagnostic {
    let path = match get_client_config_path(client) {
        Some(p) => p,
        None => {
            return ClientDiagnostic {
                client: client.to_string(),
                config_path: None,
                file_exists: false,
                tally_configured: false,
                command: None,
                command_exists: false,
                has_serve_arg: false,
                is_healthy: false,
                message: format!(
                    "Unsupported client '{}' or unable to detect OS directory",
                    client
                ),
            };
        }
    };

    if !path.exists() {
        return ClientDiagnostic {
            client: client.to_string(),
            config_path: Some(path),
            file_exists: false,
            tally_configured: false,
            command: None,
            command_exists: false,
            has_serve_arg: false,
            is_healthy: false,
            message: "Config file not found on system. Use 1-Click Auto-Configure to create it."
                .to_string(),
        };
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return ClientDiagnostic {
                client: client.to_string(),
                config_path: Some(path),
                file_exists: true,
                tally_configured: false,
                command: None,
                command_exists: false,
                has_serve_arg: false,
                is_healthy: false,
                message: format!("Failed to read config file: {}", e),
            };
        }
    };

    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            return ClientDiagnostic {
                client: client.to_string(),
                config_path: Some(path),
                file_exists: true,
                tally_configured: false,
                command: None,
                command_exists: false,
                has_serve_arg: false,
                is_healthy: false,
                message: "Config file contains invalid JSON syntax. Auto-Configure can repair it."
                    .to_string(),
            };
        }
    };

    let tally_server = val.get("mcpServers").and_then(|s| {
        s.get("tallymcp.pro")
            .or_else(|| s.get("tallymcppro"))
            .or_else(|| s.get("tally"))
    });
    if tally_server.is_none() {
        return ClientDiagnostic {
            client: client.to_string(),
            config_path: Some(path),
            file_exists: true,
            tally_configured: false,
            command: None,
            command_exists: false,
            has_serve_arg: false,
            is_healthy: false,
            message: "Config file exists, but 'tallymcp.pro' MCP server is not registered."
                .to_string(),
        };
    }

    let tally_obj = tally_server.unwrap();
    let cmd = tally_obj
        .get("command")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let args = tally_obj.get("args").and_then(|a| a.as_array());

    let has_serve = args
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("serve")))
        .unwrap_or(false);

    let cmd_exists = if let Some(ref c) = cmd {
        if c.contains('/') || c.contains('\\') {
            std::path::Path::new(c).exists()
        } else {
            true
        }
    } else {
        false
    };

    if !has_serve {
        return ClientDiagnostic {
            client: client.to_string(),
            config_path: Some(path),
            file_exists: true,
            tally_configured: true,
            command: cmd,
            command_exists: cmd_exists,
            has_serve_arg: false,
            is_healthy: false,
            message: "CRITICAL: 'args' is missing 'serve'. Claude Desktop will fail to connect via stdio!".to_string(),
        };
    }

    if let Some(c) = cmd.as_deref() {
        if (c.contains('/') || c.contains('\\')) && !cmd_exists {
            let msg = format!("Configured executable path '{}' does not exist on disk.", c);
            return ClientDiagnostic {
                client: client.to_string(),
                config_path: Some(path),
                file_exists: true,
                tally_configured: true,
                command: cmd,
                command_exists: false,
                has_serve_arg: true,
                is_healthy: false,
                message: msg,
            };
        }
    }

    ClientDiagnostic {
        client: client.to_string(),
        config_path: Some(path),
        file_exists: true,
        tally_configured: true,
        command: cmd,
        command_exists: cmd_exists,
        has_serve_arg: true,
        is_healthy: true,
        message: "Properly configured and verified.".to_string(),
    }
}
