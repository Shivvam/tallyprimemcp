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
    println!("=== end of diagnostics report ===");

    Ok(())
}
