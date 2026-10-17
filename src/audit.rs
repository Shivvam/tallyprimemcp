use crate::config::get_local_app_data_dir;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

pub struct AuditLogger {
    audit_dir: PathBuf,
    lock: Mutex<()>,
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger {
    pub fn new() -> Self {
        let base_dir = get_local_app_data_dir();
        let audit_dir = base_dir.join("audit");
        let _ = fs::create_dir_all(&audit_dir);

        Self {
            audit_dir,
            lock: Mutex::new(()),
        }
    }

    fn current_audit_path(&self) -> PathBuf {
        let now = chrono::Utc::now();
        let filename = format!("{}.jsonl", now.format("%Y-%m"));
        self.audit_dir.join(filename)
    }

    pub fn log(&self, tool: &str, draft_id: Option<&str>, company: Option<&str>, outcome: &str) {
        self.log_full(tool, draft_id, company, outcome, None);
    }

    pub fn log_full(
        &self,
        tool: &str,
        draft_id: Option<&str>,
        company: Option<&str>,
        outcome: &str,
        details: Option<&str>,
    ) {
        let _guard = self.lock.lock().unwrap();
        let path = self.current_audit_path();

        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            tool: tool.to_string(),
            draft_id: draft_id.map(|s| s.to_string()),
            company: company.map(|s| s.to_string()),
            outcome: outcome.to_string(),
            details: details.map(|s| s.to_string()),
        };

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(serialized) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{}", serialized);
            }
        }
    }

    pub fn read_recent_entries(&self, limit: usize) -> Vec<AuditEntry> {
        let _guard = self.lock.lock().unwrap();
        let path = self.current_audit_path();
        if !path.exists() {
            return Vec::new();
        }
        let Ok(content) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        let mut entries: Vec<AuditEntry> = content
            .lines()
            .filter_map(|line| serde_json::from_str::<AuditEntry>(line).ok())
            .collect();
        entries.reverse();
        entries.truncate(limit);
        entries
    }
}
