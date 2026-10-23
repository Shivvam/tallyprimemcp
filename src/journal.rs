use crate::config::get_local_app_data_dir;
use crate::models::{DraftStatus, VoucherDraft};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("I/O error in journal or draft store: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Draft with ID {0} not found.")]
    DraftNotFound(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JournalRecord {
    pub timestamp: String,
    pub event_type: String, // "draft_created", "commit_intent", "commit_result", "reconcile"
    pub draft_id: String,
    pub guid: Option<String>,
    pub content_hash: String,
    pub status: DraftStatus,
    #[serde(default)]
    pub outcome: Option<String>,
}

pub struct JournalManager {
    journal_dir: PathBuf,
    drafts_dir: PathBuf,
    lock: Mutex<()>,
}

impl JournalManager {
    pub fn new() -> Result<Self, JournalError> {
        let base_dir = get_local_app_data_dir();
        let journal_dir = base_dir.join("journal");
        let drafts_dir = base_dir.join("drafts");

        fs::create_dir_all(&journal_dir)?;
        fs::create_dir_all(&drafts_dir)?;

        Ok(Self {
            journal_dir,
            drafts_dir,
            lock: Mutex::new(()),
        })
    }

    fn current_journal_path(&self) -> PathBuf {
        let now = chrono::Utc::now();
        let filename = format!("{}.jsonl", now.format("%Y-%m-%d"));
        self.journal_dir.join(filename)
    }

    pub fn append_record(&self, record: &JournalRecord) -> Result<(), JournalError> {
        let _guard = self.lock.lock().unwrap();
        let path = self.current_journal_path();

        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let serialized = serde_json::to_string(record)?;
        writeln!(file, "{}", serialized)?;
        file.sync_all()?;
        Ok(())
    }

    pub fn save_draft(&self, draft: &VoucherDraft) -> Result<(), JournalError> {
        let _guard = self.lock.lock().unwrap();
        let filename = format!("{}.json", draft.draft_id);
        let target_path = self.drafts_dir.join(&filename);
        let temp_path = self.drafts_dir.join(format!("{}.tmp", draft.draft_id));

        // Atomic write: write to temp file, sync, then rename
        {
            let mut file = File::create(&temp_path)?;
            let serialized = serde_json::to_string_pretty(draft)?;
            file.write_all(serialized.as_bytes())?;
            file.sync_all()?;
        }

        fs::rename(temp_path, target_path)?;
        Ok(())
    }

    pub fn get_draft(&self, draft_id: &str) -> Result<VoucherDraft, JournalError> {
        let target_path = self.drafts_dir.join(format!("{}.json", draft_id));
        if !target_path.exists() {
            return Err(JournalError::DraftNotFound(draft_id.to_string()));
        }

        let content = fs::read_to_string(target_path)?;
        let draft = serde_json::from_str::<VoucherDraft>(&content)?;
        Ok(draft)
    }

    pub fn list_pending_drafts(&self) -> Result<Vec<VoucherDraft>, JournalError> {
        let mut drafts = Vec::new();
        if !self.drafts_dir.exists() {
            return Ok(drafts);
        }

        for entry in fs::read_dir(&self.drafts_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(draft) = serde_json::from_str::<VoucherDraft>(&content) {
                        // Do not return drafts that have already been committed to Tally or discarded
                        if draft.status != DraftStatus::Committed
                            && draft.status != DraftStatus::Discarded
                        {
                            drafts.push(draft);
                        }
                    }
                }
            }
        }

        Ok(drafts)
    }

    #[allow(dead_code)]
    pub fn record_commit_intent(&self, draft: &VoucherDraft) -> Result<(), JournalError> {
        let now = chrono::Utc::now().to_rfc3339();
        let record = JournalRecord {
            timestamp: now,
            event_type: "commit_intent".to_string(),
            draft_id: draft.draft_id.clone(),
            guid: draft.tally_guid.clone(),
            content_hash: draft.content_hash.clone(),
            status: DraftStatus::Committing,
            outcome: None,
        };
        self.append_record(&record)?;

        let mut updated_draft = draft.clone();
        updated_draft.status = DraftStatus::Committing;
        self.save_draft(&updated_draft)?;
        Ok(())
    }

    pub fn record_commit_result(
        &self,
        draft: &VoucherDraft,
        status: DraftStatus,
        outcome: &str,
    ) -> Result<(), JournalError> {
        let now = chrono::Utc::now().to_rfc3339();
        let record = JournalRecord {
            timestamp: now,
            event_type: "commit_result".to_string(),
            draft_id: draft.draft_id.clone(),
            guid: draft.tally_guid.clone(),
            content_hash: draft.content_hash.clone(),
            status: status.clone(),
            outcome: Some(outcome.to_string()),
        };
        self.append_record(&record)?;

        let mut updated_draft = draft.clone();
        updated_draft.status = status;
        self.save_draft(&updated_draft)?;
        Ok(())
    }

    pub fn reconcile_startup(&self) -> Result<Vec<String>, JournalError> {
        let drafts = self.list_pending_drafts()?;
        let mut reconciled = Vec::new();

        for mut draft in drafts {
            if draft.status == DraftStatus::Committing {
                // Left in committing state due to abrupt shutdown
                eprintln!("[tally-mcp journal] Found interrupted draft {} in 'committing' state. Reconciling...", draft.draft_id);
                // In accordance with §6.8, revert to approved for explicit human retry (never auto-post)
                draft.status = DraftStatus::Approved;
                self.save_draft(&draft)?;

                let now = chrono::Utc::now().to_rfc3339();
                let record = JournalRecord {
                    timestamp: now,
                    event_type: "reconcile".to_string(),
                    draft_id: draft.draft_id.clone(),
                    guid: draft.tally_guid.clone(),
                    content_hash: draft.content_hash.clone(),
                    status: DraftStatus::Approved,
                    outcome: Some("Reverted interrupted commit to approved status".to_string()),
                };
                self.append_record(&record)?;
                reconciled.push(draft.draft_id);
            }
        }

        Ok(reconciled)
    }
}
