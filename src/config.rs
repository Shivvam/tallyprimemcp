use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub tally: TallyConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub packs: PacksConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub cleanup: CleanupConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tally: TallyConfig::default(),
            safety: SafetyConfig::default(),
            ai: AiConfig::default(),
            packs: PacksConfig::default(),
            ui: UiConfig::default(),
            logging: LoggingConfig::default(),
            cleanup: CleanupConfig::default(),
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
    pub base_url: String,
    pub extra_allowed_hosts: Vec<String>,
    pub send_ledger_names_for_matching: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            profile: "cloud".to_string(),
            provider: "anthropic".to_string(),
            model: String::new(),
            base_url: String::new(),
            extra_allowed_hosts: Vec::new(),
            send_ledger_names_for_matching: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PacksConfig {
    pub enable_unsigned: bool,
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiConfig {
    pub auto_open_ui: bool,
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

    // Environment variable overrides (supports connecting to TallyPrime over LAN)
    if let Ok(host) = std::env::var("TALLY_HOST") {
        let trimmed = host.trim();
        if !trimmed.is_empty() {
            config.tally.host = trimmed.to_string();
            config.tally.allow_remote_tally = true;
        }
    }
    if let Ok(port_str) = std::env::var("TALLY_PORT") {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            config.tally.port = port;
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
        "tally".to_string(),
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

    let tally_server = val.get("mcpServers").and_then(|s| s.get("tally"));
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
            message: "Config file exists, but 'tally' MCP server is not registered.".to_string(),
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
