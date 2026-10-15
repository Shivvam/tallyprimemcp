#![allow(dead_code)]
use crate::config::get_local_app_data_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionHistoryEntry {
    pub version: String,
    pub installed_at: String,
    pub install_source: String, // "msi" | "msix" | "dev"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallManifest {
    pub installation_id: String,
    pub install_source: String,
    pub current_version: String,
    pub version_history: Vec<VersionHistoryEntry>,
    pub owned_paths: Vec<String>,
    pub client_configs_modified: Vec<String>,
}

impl InstallManifest {
    pub fn get_manifest_path() -> PathBuf {
        get_local_app_data_dir().join("install-manifest.json")
    }

    pub fn load_or_create(install_source: &str) -> Self {
        let path = Self::get_manifest_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(manifest) = serde_json::from_str::<InstallManifest>(&content) {
                    return manifest;
                }
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        let version = env!("CARGO_PKG_VERSION").to_string();

        let base_local = get_local_app_data_dir();
        let base_roaming = crate::config::get_app_data_dir();

        let owned_paths = vec![
            base_local.to_string_lossy().to_string(),
            base_roaming.to_string_lossy().to_string(),
        ];

        Self {
            installation_id: uuid::Uuid::new_v4().to_string(),
            install_source: install_source.to_string(),
            current_version: version.clone(),
            version_history: vec![VersionHistoryEntry {
                version,
                installed_at: now,
                install_source: install_source.to_string(),
            }],
            owned_paths,
            client_configs_modified: Vec::new(),
        }
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::get_manifest_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}
