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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match subcommand {
        "serve" => {
            let (config, _) = load_config();
            let server = McpServer::new(config);
            server.run_stdio().await?;
        }

        "doctor" => {
            let (config, _) = load_config();
            let show_names = args.iter().any(|a| a == "--show-names");
            doctor::run_doctor(&config, show_names).await?;
        }

        "version" => {
            println!("tally-mcp {}", env!("CARGO_PKG_VERSION"));
            println!("Build Hash: {}", option_env!("GIT_HASH").unwrap_or("local-build"));
            println!("Target OS: {}", std::env::consts::OS);
            println!("Target Architecture: {}", std::env::consts::ARCH);
            println!("Schema Version: 1");
        }

        "setup" => {
            println!("=== tally-mcp Setup Wizard ===");
            let (config, config_path) = load_config();
            println!("Target config location: {}", config_path.display());
            println!("Current Tally host: {}:{}", config.tally.host, config.tally.port);
            println!("AI Profile: {}", config.ai.profile);
            println!("\nTo configure Claude Desktop, add the following to your claude_desktop_config.json:");
            println!(r#"  "tally": {{ "command": "tally-mcp", "args": ["serve"] }}"#);
        }

        "cleanup" => {
            let dry_run = args.iter().any(|a| a == "--dry-run");
            let force = args.iter().any(|a| a == "--force");

            let backup_dir = args.iter().position(|a| a == "--backup")
                .and_then(|idx| args.get(idx + 1))
                .map(std::path::PathBuf::from);

            let categories = args.iter().position(|a| a == "--categories")
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
            let (config, _) = load_config();
            let ui_server = ui::CompanionUiServer::new(config);
            let port = 9190;
            ui_server.run(port)?;
        }

        "help" | "-h" | "--help" | _ => {
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("tally-mcp v{} — Safe, extensible Model Context Protocol server for TallyPrime", env!("CARGO_PKG_VERSION"));
    println!("\nUsage: tally-mcp <COMMAND> [OPTIONS]");
    println!("\nCommands:");
    println!("  serve     Start headless stdio MCP server for AI clients (Claude Desktop, etc.)");
    println!("  doctor    Run connectivity and configuration diagnostics");
    println!("  setup     Interactive CLI wizard for client integration");
    println!("  ui        Launch loopback companion web interface in browser");
    println!("  cleanup   Manage and clean local application state");
    println!("  version   Print version, build target, and schema information");
}
