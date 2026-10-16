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
            if selected_cats.contains(&item.category.as_str()) && item.category != "credentials" {
                if item.path.exists() {
                    let dest = backup_folder.join(&item.category);
                    copy_dir_recursive(&item.path, &dest)?;
                    println!("  Backed up: {}", item.category);
                }
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

    println!("\nCleanup completed cleanly.");
    Ok(())
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
