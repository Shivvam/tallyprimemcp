use crate::config::Config;
use crate::tally::TallyClient;

pub async fn run_doctor(
    config: &Config,
    show_names: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== tally-mcp doctor diagnostics ===");
    println!("App Version: {}", env!("CARGO_PKG_VERSION"));
    println!(
        "Build Hash: {}",
        option_env!("GIT_HASH").unwrap_or("release-build")
    );
    println!("OS: {}", std::env::consts::OS);
    println!("Architecture: {}", std::env::consts::ARCH);
    println!("Configuration Source: loaded");
    println!(
        "Tally Target: http://{}:{}",
        config.tally.host, config.tally.port
    );
    println!("Write Mode: {}", config.safety.write_mode);
    println!("Queue Depth: {}", config.tally.queue_depth);
    println!(
        "Breaker Threshold: {} failures",
        config.tally.breaker_failures
    );

    // Test Tally gateway reachability
    let client = TallyClient::new(config.tally.clone());
    match client.status().await {
        Ok(status) => {
            println!("Tally Reachability: OK (reachable)");
            println!("Circuit Breaker: {}", status.circuit_breaker);
            println!("Loaded Companies Count: {}", status.loaded_companies.len());
            for (idx, comp) in status.loaded_companies.iter().enumerate() {
                if show_names {
                    println!("  [{}] {}", idx + 1, comp);
                } else {
                    let masked = if comp.len() > 3 {
                        format!("{}***", &comp[..3])
                    } else {
                        "***".to_string()
                    };
                    println!("  [{}] {}", idx + 1, masked);
                }
            }
        }
        Err(e) => {
            println!("Tally Reachability: FAILED ({})", e);
            println!(
                "Guidance: Ensure TallyPrime is running and HTTP Server is enabled on port {}.",
                config.tally.port
            );
        }
    }

    println!("AI Profile: {}", config.ai.profile);
    println!("AI Provider: {}", config.ai.provider);
    println!("Egress Allow-list: built-in cloud hosts, Tally host");

    // MCP Client Integration Diagnostics
    println!("\n=== MCP Client Integration Diagnostics ===");
    for client_name in &["claude", "cursor", "windsurf"] {
        let diag = crate::config::diagnose_client_config(client_name);
        let status_label = if diag.is_healthy {
            "[✓ OK]"
        } else if diag.tally_configured {
            "[! WARN]"
        } else {
            "[? NOT CONFIGURED]"
        };

        println!(
            "{} {}: {}",
            status_label,
            client_name.to_uppercase(),
            diag.message
        );
        if let Some(ref path) = diag.config_path {
            println!("  • Config path: {}", path.display());
        }
        if let Some(ref cmd) = diag.command {
            println!("  • Command:     {}", cmd);
            println!(
                "  • Exe on disk: {}",
                if diag.command_exists {
                    "Found"
                } else {
                    "NOT FOUND (Broken Path)"
                }
            );
        }
        if diag.tally_configured {
            println!(
                "  • 'serve' arg: {}",
                if diag.has_serve_arg {
                    "Present"
                } else {
                    "MISSING (Required for stdio MCP)"
                }
            );
        }
    }
    println!("=== end of diagnostics report ===");

    Ok(())
}
