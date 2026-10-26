mod audit;
mod byok;
mod cleanup;
mod config;
mod doctor;
mod journal;
mod manifest;
mod mcp;
mod models;
mod packs;
mod tally;
mod ui;
mod validator;

use config::load_config;
use mcp::McpServer;
use std::env;

const TOOLS_META_JSON: &str = include_str!("../tools/meta.json");

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    // Default to launching Companion UI if no subcommand is passed or if options are passed directly
    let subcommand = match args.get(1).map(|s| s.as_str()) {
        None => "ui",
        Some(s)
            if s.starts_with("--host")
                || s.starts_with("--port")
                || s.starts_with("--tally-")
                || s.starts_with("--ui-port") =>
        {
            "ui"
        }
        Some(s) => s,
    };

    // Extract CLI overrides: --host <IP/HOSTNAME>, --port <PORT>, and --ui-port <PORT>
    let mut cli_host: Option<String> = None;
    let mut cli_port: Option<u16> = None;
    let mut cli_ui_port: Option<u16> = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--host" || args[i] == "--tally-host" {
            if let Some(val) = args.get(i + 1) {
                cli_host = Some(val.clone());
                i += 1;
            }
        } else if args[i].starts_with("--host=") {
            cli_host = Some(args[i].trim_start_matches("--host=").to_string());
        } else if args[i].starts_with("--tally-host=") {
            cli_host = Some(args[i].trim_start_matches("--tally-host=").to_string());
        } else if args[i] == "--port" || args[i] == "--tally-port" {
            if let Some(val) = args.get(i + 1) {
                if let Ok(p) = val.parse::<u16>() {
                    cli_port = Some(p);
                }
                i += 1;
            }
        } else if args[i].starts_with("--port=") {
            if let Ok(p) = args[i].trim_start_matches("--port=").parse::<u16>() {
                cli_port = Some(p);
            }
        } else if args[i].starts_with("--tally-port=") {
            if let Ok(p) = args[i].trim_start_matches("--tally-port=").parse::<u16>() {
                cli_port = Some(p);
            }
        } else if args[i] == "--ui-port" {
            if let Some(val) = args.get(i + 1) {
                if let Ok(p) = val.parse::<u16>() {
                    cli_ui_port = Some(p);
                }
                i += 1;
            }
        } else if args[i].starts_with("--ui-port=") {
            if let Ok(p) = args[i].trim_start_matches("--ui-port=").parse::<u16>() {
                cli_ui_port = Some(p);
            }
        }
        i += 1;
    }

    match subcommand {
        "tools" => {
            let md_mode = args.iter().any(|a| a == "--markdown");
            if md_mode {
                let tools: Vec<serde_json::Value> =
                    serde_json::from_str(TOOLS_META_JSON).unwrap_or_default();
                println!("| Tool | Access | Toolset | Summary |");
                println!("|---|---|---|---|");
                for t in tools {
                    let name = t["name"].as_str().unwrap_or("");
                    let access = t["access"].as_str().unwrap_or("");
                    let toolset = t["toolset"].as_str().unwrap_or("");
                    let summary = t["summary"].as_str().unwrap_or("").replace('|', "\\|");
                    println!("| `{}` | {} | {} | {} |", name, access, toolset, summary);
                }
            } else {
                println!("{}", TOOLS_META_JSON.trim());
            }
            return Ok(());
        }

        "serve" => {
            let (mut config, _) = load_config();
            if let Some(host) = cli_host {
                config.tally.host = host;
                config.tally.allow_remote_tally = true;
            }
            if let Some(port) = cli_port {
                config.tally.port = port;
            }
            let server = McpServer::new(config);
            server.run_stdio().await?;
        }

        "doctor" => {
            let (mut config, _) = load_config();
            if let Some(host) = cli_host {
                config.tally.host = host;
                config.tally.allow_remote_tally = true;
            }
            if let Some(port) = cli_port {
                config.tally.port = port;
            }
            let show_names = args.iter().any(|a| a == "--show-names");
            doctor::run_doctor(&config, show_names).await?;
        }

        "version" | "--version" | "-v" => {
            println!("tally-mcp {}", env!("CARGO_PKG_VERSION"));
            println!(
                "Build Hash: {}",
                option_env!("GIT_HASH").unwrap_or("local-build")
            );
            println!("Target OS: {}", std::env::consts::OS);
            println!("Target Architecture: {}", std::env::consts::ARCH);
            println!("Schema Version: 1");
        }

        "setup" => {
            println!("=== tally-mcp Setup Wizard ===");
            let (mut config, config_path) = load_config();

            if let Some(ref h) = cli_host {
                config.tally.host = h.clone();
                config.tally.allow_remote_tally = true;
            }
            if let Some(p) = cli_port {
                config.tally.port = p;
            }

            if cli_host.is_some() || cli_port.is_some() {
                if let Some(parent) = config_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(serialized) = toml::to_string_pretty(&config) {
                    if std::fs::write(&config_path, serialized).is_ok() {
                        println!("Configuration saved to: {}", config_path.display());
                    }
                }
            }

            println!("Config file location: {}", config_path.display());
            println!(
                "Current Tally target: http://{}:{}",
                config.tally.host, config.tally.port
            );
            println!("AI Profile: {}", config.ai.profile);

            // Automatically configure Claude Desktop with exact executable path
            match crate::config::auto_configure_client(
                "claude",
                &config.tally.host,
                config.tally.port,
            ) {
                Ok((path, exe)) => {
                    println!("\n[✓] Claude Desktop automatically configured!");
                    println!("    Config file: {}", path.display());
                    println!("    Executable:  {}", exe);
                    println!("    Arguments:   [\"serve\"]");
                    println!("    Action required: Fully restart Claude Desktop (Quit from system tray icon).");
                }
                Err(e) => {
                    println!("\n[!] Claude Desktop auto-configuration skipped: {}", e);
                }
            }

            // Create desktop shortcut for Companion UI
            match crate::cleanup::create_desktop_shortcut() {
                Ok(path) => println!("[✓] Desktop shortcut created: {}", path.display()),
                Err(e) => println!("[!] Desktop shortcut skipped: {}", e),
            }

            println!("\nLAN Support Tips:");
            println!(
                "  • Connect to remote/LAN TallyPrime: tally-mcp serve --host <LAN_IP> --port 9000"
            );
            println!("  • Or set environment variable:      TALLY_HOST=<LAN_IP>");
            println!(
                "  • Or save permanently:              tally-mcp setup --host <LAN_IP> --port 9000"
            );
        }

        "uninstall" => {
            let dry_run = args.iter().any(|a| a == "--dry-run");
            println!("=== tally-mcp Complete Clean Uninstall ===");
            let opts = cleanup::CleanupOptions {
                dry_run,
                categories: vec![],
                backup_dir: None,
                force: true,
            };
            cleanup::execute_cleanup(&opts)?;
            if !dry_run {
                println!("All TallyMCP application data, logs, credentials, PATH entries, and shortcuts have been cleanly removed.");
            }
        }

        "cleanup" => {
            let dry_run = args.iter().any(|a| a == "--dry-run");
            let force = args.iter().any(|a| a == "--force");

            let backup_dir = args
                .iter()
                .position(|a| a == "--backup")
                .and_then(|idx| args.get(idx + 1))
                .map(std::path::PathBuf::from);

            let categories = args
                .iter()
                .position(|a| a == "--categories")
                .and_then(|idx| args.get(idx + 1))
                .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
                .unwrap_or_default();

            let opts = cleanup::CleanupOptions {
                dry_run,
                categories,
                backup_dir,
                force,
            };

            cleanup::execute_cleanup(&opts)?;
        }

        "ui" => {
            let (mut config, _) = load_config();
            if let Some(host) = cli_host {
                config.tally.host = host;
                config.tally.allow_remote_tally = true;
            }
            if let Some(port) = cli_port {
                config.tally.port = port;
            }
            let ui_server = ui::CompanionUiServer::new(config);
            let port = cli_ui_port.unwrap_or(9190);
            ui_server.run(port)?;
        }

        "help" | "-h" | "--help" => {
            print_usage();
        }
        _ => {
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!(
        "tally-mcp v{} — Safe, extensible Model Context Protocol server for TallyPrime",
        env!("CARGO_PKG_VERSION")
    );
    println!("\nUsage: tally-mcp [COMMAND] [OPTIONS]");
    println!("\nCommands:");
    println!("  ui        Launch loopback companion web interface in browser (default)");
    println!("  serve     Start headless stdio MCP server for AI clients (Claude Desktop, etc.)");
    println!("  doctor    Run connectivity and configuration diagnostics");
    println!("  setup     Interactive CLI wizard for client integration");
    println!("  cleanup   Manage and clean local application state");
    println!(
        "  uninstall Cleanly uninstall all app data, shortcuts, PATH, and client integrations"
    );
    println!("  version   Print version, build target, and schema information");
    println!("  help      Print this usage information");
    println!("\nConnection Options (Local or LAN):");
    println!("  --host <IP/HOST>   TallyPrime target host (default: 127.0.0.1; e.g. 192.168.1.50)");
    println!("  --port <PORT>      TallyPrime XML port (default: 9000)");
    println!("  --ui-port <PORT>   Companion UI server port (default: 9190)");
}
