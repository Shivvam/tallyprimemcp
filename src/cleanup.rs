use crate::config::{get_app_data_dir, get_local_app_data_dir};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CleanupOptions {
    pub dry_run: bool,
    pub categories: Vec<String>, // "settings", "drafts", "audit", "logs", "credentials"
    pub backup_dir: Option<PathBuf>,
    #[allow(dead_code)]
    pub force: bool,
}

pub struct CleanupInventoryItem {
    pub category: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub file_count: usize,
    pub note: String,
}

pub fn get_cleanup_inventory() -> Vec<CleanupInventoryItem> {
    let app_data = get_app_data_dir();
    let local_data = get_local_app_data_dir();

    vec![
        calculate_dir_stats(
            "settings",
            &app_data,
            "Settings & tool packs (config.toml, tools/*.json). Quick to re-create.",
        ),
        calculate_dir_stats(
            "drafts",
            &local_data.join("drafts"),
            "Pending voucher drafts and write-ahead journal.",
        ),
        calculate_dir_stats(
            "audit",
            &local_data.join("audit"),
            "Audit log trail (audit/*.jsonl). Accounting evidence; backup recommended.",
        ),
        calculate_dir_stats(
            "logs",
            &local_data.join("logs"),
            "Application diagnostic logs and temporary caches.",
        ),
        CleanupInventoryItem {
            category: "credentials".to_string(),
            path: PathBuf::from("Windows Credential Manager: TallyMCP/*"),
            size_bytes: 0,
            file_count: 0,
            note: "Stored API keys. Keys are not exportable and must be re-entered if removed."
                .to_string(),
        },
    ]
}

fn calculate_dir_stats(category: &str, dir: &Path, note: &str) -> CleanupInventoryItem {
    let mut total_size = 0;
    let mut file_count = 0;

    if dir.exists() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        total_size += meta.len();
                        file_count += 1;
                    }
                }
            }
        }
    }

    CleanupInventoryItem {
        category: category.to_string(),
        path: dir.to_path_buf(),
        size_bytes: total_size,
        file_count,
        note: note.to_string(),
    }
}

pub fn execute_cleanup(options: &CleanupOptions) -> Result<(), Box<dyn std::error::Error>> {
    let inventory = get_cleanup_inventory();

    println!("=== tally-mcp Lifecycle & Data Cleanup ===");

    if options.dry_run {
        println!("[DRY RUN MODE] The following categories were inventoried:\n");
        for item in &inventory {
            println!(
                "Category: {:<12} | Files: {:<4} | Size: {:<10} bytes\nPath: {}\nNote: {}\n",
                item.category,
                item.file_count,
                item.size_bytes,
                item.path.display(),
                item.note
            );
        }
        println!("No files, credentials, or registry keys were modified.");
        return Ok(());
    }

    // Determine target categories
    let selected_cats = if options.categories.is_empty() {
        vec!["settings", "drafts", "audit", "logs", "credentials"]
    } else {
        options.categories.iter().map(|s| s.as_str()).collect()
    };

    // Backup step if requested
    if let Some(backup_dest) = &options.backup_dir {
        fs::create_dir_all(backup_dest)?;
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_folder = backup_dest.join(format!("tallymcp_backup_{}", timestamp));
        fs::create_dir_all(&backup_folder)?;

        println!(
            "Backing up non-secret categories to: {}",
            backup_folder.display()
        );
        for item in &inventory {
            if selected_cats.contains(&item.category.as_str())
                && item.category != "credentials"
                && item.path.exists()
            {
                let dest = backup_folder.join(&item.category);
                copy_dir_recursive(&item.path, &dest)?;
                println!("  Backed up: {}", item.category);
            }
        }
    }

    // Deletion step: strictly confined to allow-listed owned directories
    for item in &inventory {
        if selected_cats.contains(&item.category.as_str()) {
            match item.category.as_str() {
                "settings" | "drafts" | "audit" | "logs" => {
                    if item.path.exists() {
                        let _ = fs::remove_dir_all(&item.path);
                        println!("Removed directory: {}", item.path.display());
                    }
                }
                "credentials" => {
                    #[cfg(feature = "byok")]
                    {
                        for provider in &["anthropic", "gemini", "openai"] {
                            if let Ok(entry) = keyring::Entry::new("TallyMCP", provider) {
                                let _ = entry.delete_credential();
                            }
                        }
                        println!("Removed API keys from Credential Manager (prefix: TallyMCP/*)");
                    }
                }
                _ => {}
            }
        }
    }

    // When doing a full/force cleanup, clean parent folders, shortcuts, PATH, and client integrations
    if options.force || options.categories.is_empty() {
        let app_data = get_app_data_dir();
        if app_data.exists() {
            let _ = fs::remove_dir_all(&app_data);
            println!(
                "Removed application configuration folder: {}",
                app_data.display()
            );
        }

        let local_data = get_local_app_data_dir();
        if local_data.exists() {
            let _ = fs::remove_dir_all(&local_data);
            println!(
                "Removed local application data folder: {}",
                local_data.display()
            );
        }

        remove_desktop_shortcut();
        remove_from_user_path();
        remove_client_integrations();
    }

    println!("\nCleanup completed cleanly.");
    Ok(())
}

/// Creates a desktop icon/shortcut for TallyMCP Companion UI
pub fn create_desktop_shortcut() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot locate current executable path: {}", e))?;

    #[cfg(target_os = "windows")]
    {
        let desktop = std::env::var("USERPROFILE")
            .map(|u| PathBuf::from(u).join("Desktop"))
            .map_err(|_| "USERPROFILE environment variable not found".to_string())?;

        if !desktop.exists() {
            let _ = fs::create_dir_all(&desktop);
        }

        let lnk_path = desktop.join("TallyMCP.lnk");
        let script = format!(
            "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.Arguments = 'ui'; $s.Description = 'TallyMCP Companion & Server'; $s.WorkingDirectory = '{}'; $s.Save()",
            lnk_path.display(),
            current_exe.display(),
            current_exe.parent().unwrap_or(&current_exe).display()
        );

        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output();

        match status {
            Ok(output) if output.status.success() => {
                println!("Created desktop shortcut: {}", lnk_path.display());
                Ok(lnk_path)
            }
            _ => {
                // Fallback: create .url Internet Shortcut pointing to localhost UI
                let url_path = desktop.join("TallyMCP.url");
                let url_content = format!(
                    "[InternetShortcut]\nURL=http://127.0.0.1:9190\nIconIndex=0\nIconFile={}\n",
                    current_exe.display()
                );
                fs::write(&url_path, url_content)
                    .map_err(|e| format!("Failed to create fallback desktop shortcut: {}", e))?;
                println!("Created desktop shortcut: {}", url_path.display());
                Ok(url_path)
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME directory not found".to_string())?;
        let desktop = PathBuf::from(&home).join("Desktop");
        let apps_dir = PathBuf::from(&home).join(".local/share/applications");

        let entry = format!(
            "[Desktop Entry]\nVersion=1.0\nType=Application\nName=TallyMCP\nComment=TallyModel Context Protocol Companion & Server\nExec={} ui\nTerminal=false\nCategories=Office;Finance;\n",
            current_exe.display()
        );

        let mut created_path = desktop.join("tally-mcp.desktop");
        if desktop.exists() {
            let _ = fs::write(&created_path, &entry);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&created_path, fs::Permissions::from_mode(0o755));
            }
            println!("Created desktop shortcut: {}", created_path.display());
        }

        if apps_dir.exists() {
            let app_shortcut = apps_dir.join("tally-mcp.desktop");
            let _ = fs::write(&app_shortcut, &entry);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&app_shortcut, fs::Permissions::from_mode(0o755));
            }
            created_path = app_shortcut;
        }

        Ok(created_path)
    }
}

/// Removes desktop and start menu shortcuts cleanly
pub fn remove_desktop_shortcut() {
    #[cfg(target_os = "windows")]
    {
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            let desktop = PathBuf::from(&user_profile).join("Desktop");
            let lnk = desktop.join("TallyMCP.lnk");
            let url = desktop.join("TallyMCP.url");
            if lnk.exists() {
                let _ = fs::remove_file(&lnk);
                println!("Removed desktop shortcut: {}", lnk.display());
            }
            if url.exists() {
                let _ = fs::remove_file(&url);
                println!("Removed desktop shortcut: {}", url.display());
            }
        }

        if let Ok(public) = std::env::var("PUBLIC") {
            let pub_desktop = PathBuf::from(&public).join("Desktop");
            let lnk = pub_desktop.join("TallyMCP.lnk");
            if lnk.exists() {
                let _ = fs::remove_file(&lnk);
                println!("Removed public desktop shortcut: {}", lnk.display());
            }
        }

        if let Ok(app_data) = std::env::var("APPDATA") {
            let start_menu_folder = PathBuf::from(&app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("TallyMCP");
            if start_menu_folder.exists() {
                let _ = fs::remove_dir_all(&start_menu_folder);
                println!("Removed Start Menu folder: {}", start_menu_folder.display());
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let desk_file = PathBuf::from(&home)
                .join("Desktop")
                .join("tally-mcp.desktop");
            if desk_file.exists() {
                let _ = fs::remove_file(&desk_file);
                println!("Removed desktop shortcut: {}", desk_file.display());
            }
            let app_file = PathBuf::from(&home)
                .join(".local/share/applications")
                .join("tally-mcp.desktop");
            if app_file.exists() {
                let _ = fs::remove_file(&app_file);
                println!("Removed applications shortcut: {}", app_file.display());
            }
        }
    }
}

/// Removes any TallyMCP directory from the User PATH environment variable
pub fn remove_from_user_path() {
    #[cfg(target_os = "windows")]
    {
        let script = "$p = [Environment]::GetEnvironmentVariable('Path', 'User'); if ($p) { $np = ($p -split ';' | Where-Object { $_ -and $_ -notmatch '(?i)TallyMCP' }) -join ';'; [Environment]::SetEnvironmentVariable('Path', $np, 'User') }";
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();
        if output.is_ok() {
            println!("Cleaned TallyMCP entries from User PATH environment variable.");
        }
    }
}

/// Removes 'tally' registrations from MCP client config files (Claude, Cursor, Cline, Windsurf)
pub fn remove_client_integrations() {
    for client in &["claude", "cursor", "cline", "windsurf"] {
        if let Some(path) = crate::config::get_client_config_path(client) {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(servers) =
                            val.get_mut("mcpServers").and_then(|s| s.as_object_mut())
                        {
                            let removed = servers.remove("tallymcp.pro").is_some()
                                || servers.remove("tallymcppro").is_some()
                                || servers.remove("tally").is_some();
                            if removed {
                                if let Ok(serialized) = serde_json::to_string_pretty(&val) {
                                    let _ = fs::write(&path, serialized);
                                    println!(
                                        "Removed TallyMCP registration from {} ({})",
                                        client,
                                        path.display()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
