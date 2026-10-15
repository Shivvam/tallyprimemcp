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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacksConfig {
    pub enable_unsigned: bool,
    pub disabled: Vec<String>,
}

impl Default for PacksConfig {
    fn default() -> Self {
        Self {
            enable_unsigned: false,
            disabled: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub auto_open_ui: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            auto_open_ui: false,
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
        PathBuf::from(home).join(".local").join("share").join("TallyMCP")
    } else {
        PathBuf::from(".tallymcp_local")
    }
}

pub fn load_config() -> (Config, PathBuf) {
    let config_dir = get_app_data_dir();
    let config_path = config_dir.join("config.toml");

    let mut config = if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(cfg) = toml::from_str::<Config>(&content) {
                cfg
            } else {
                Config::default()
            }
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

    let serialized = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    std::fs::write(&config_path, serialized)?;

    Ok(config_path)
}
