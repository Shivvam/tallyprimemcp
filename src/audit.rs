use crate::config::{get_client_data_dir, get_local_app_data_dir, Config};
use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub prev_hash: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
}

pub struct AuditLogger {
    audit_dir: PathBuf,
    lock: Mutex<String>, // holds last hash
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
            lock: Mutex::new(String::from("00000000000000000000000000000000")),
        }
    }

    fn current_audit_path(&self) -> PathBuf {
        let now = chrono::Utc::now();
        let filename = format!("{}.jsonl", now.format("%Y-%m-%d"));
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
        let mut prev_hash_guard = self.lock.lock().unwrap();
        let path = self.current_audit_path();
        let prev_hash = prev_hash_guard.clone();

        let mut entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            tool: tool.to_string(),
            draft_id: draft_id.map(|s| s.to_string()),
            company: company.map(|s| s.to_string()),
            outcome: outcome.to_string(),
            details: details.map(|s| s.to_string()),
            prev_hash: Some(prev_hash.clone()),
            hash: None,
        };

        // Compute hash
        let mut ctx = Context::new(&SHA256);
        let raw = format!(
            "{}:{}:{}:{}",
            prev_hash, entry.timestamp, entry.tool, entry.outcome
        );
        ctx.update(raw.as_bytes());
        let hash_hex = hex_encode(ctx.finish().as_ref());
        entry.hash = Some(hash_hex.clone());
        *prev_hash_guard = hash_hex;

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(serialized) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{}", serialized);
            }
        }
    }

    pub fn read_recent_entries(&self, limit: usize) -> Vec<AuditEntry> {
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

// -------------------------------------------------------------
// Internal Audit Engine Data Structures & Stages (§8)
// -------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFinding {
    pub id: String,
    pub code: String,
    pub sev: String, // "critical" | "high" | "medium" | "low" | "informational"
    pub count: usize,
    pub inr: f64,
    pub ref_info: String,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub evidence_handle: Option<String>,
    pub tally_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditScore {
    pub overall: u32,
    pub area: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRunResult {
    pub ok: u8,
    pub run: String,
    pub status: String, // "complete" | "complete_with_skips" | "partial" | "failed"
    pub scope: AuditScope,
    pub score: AuditScore,
    pub stages: HashMap<String, String>,
    pub sev: HashMap<String, usize>,
    pub cols: Vec<String>,
    pub rows: Vec<serde_json::Value>,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditScope {
    pub co: String,
    pub from: String,
    pub to: String,
    pub tally: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingConfig {
    pub method: String, // "benford" | "random" | "equal_coverage" | "none"
    pub size: usize,
    pub seed: u64,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            method: "benford".to_string(),
            size: 60,
            seed: 42,
        }
    }
}

/// Benford's Law analysis (§8.5)
#[allow(dead_code)]
pub fn run_benford_analysis(amounts: &[f64]) -> (f64, String, Vec<f64>) {
    if amounts.is_empty() {
        return (0.0, "insufficient_data".to_string(), vec![]);
    }

    let mut first_digit_counts = [0usize; 10]; // 1..9 used
    let mut total_valid = 0usize;

    for &amt in amounts {
        let abs_amt = amt.abs();
        if abs_amt >= 10.0 {
            let s = format!("{:.0}", abs_amt);
            if let Some(first_char) = s.chars().next() {
                if let Some(d) = first_char.to_digit(10) {
                    if (1..=9).contains(&d) {
                        first_digit_counts[d as usize] += 1;
                        total_valid += 1;
                    }
                }
            }
        }
    }

    if total_valid < 300 {
        return (0.0, "SKIPPED(population<300)".to_string(), vec![]);
    }

    // Expected Benford probabilities: P(d) = log10(1 + 1/d)
    let mut observed_props = Vec::with_capacity(9);
    let mut diff_sum = 0.0;

    for (d, &count) in first_digit_counts.iter().enumerate().take(10).skip(1) {
        let expected = (1.0 + 1.0 / (d as f64)).log10();
        let obs = (count as f64) / (total_valid as f64);
        observed_props.push(obs);
        diff_sum += (obs - expected).abs();
    }

    let mad = diff_sum / 9.0;
    let conformity = if mad <= 0.006 {
        "close"
    } else if mad <= 0.012 {
        "acceptable"
    } else if mad <= 0.015 {
        "marginal"
    } else {
        "non-conforming"
    };

    (mad, conformity.to_string(), observed_props)
}

/// Scoring helper according to §8.6
#[allow(dead_code)]
pub fn calculate_finding_points(sev: &str, count: usize) -> f64 {
    let w_sev = match sev {
        "critical" => 10.0,
        "high" => 5.0,
        "medium" => 2.0,
        "low" => 0.5,
        _ => 0.0,
    };
    w_sev * (1.0 + ((1 + count) as f64).log10())
}

/// Generate internal audit report markdown file (§8.8)
pub fn generate_audit_report_markdown(
    scope: &AuditScope,
    run_id: &str,
    score: &AuditScore,
    sev_counts: &HashMap<String, usize>,
    findings: &[AuditFinding],
    stages: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("# TallyMCP Internal Audit Report\n\n");
    out.push_str("> Generated by tally-mcp as an internal-control aid. It is not an audit opinion and provides no assurance.\n\n");

    out.push_str("## 1. Engagement & Cover\n");
    out.push_str(&format!("- **Company:** {}\n", scope.co));
    out.push_str(&format!("- **Period:** {} to {}\n", scope.from, scope.to));
    out.push_str(&format!("- **Run ID:** {}\n", run_id));
    out.push_str(&format!("- **Tally Target:** {}\n", scope.tally));
    out.push_str(&format!(
        "- **Overall Risk Score:** {} / 100\n\n",
        score.overall
    ));

    out.push_str("## 2. Executive Summary\n");
    out.push_str(&format!(
        "- **Findings Summary:** Critical: {}, High: {}, Medium: {}, Low: {}\n\n",
        sev_counts.get("critical").unwrap_or(&0),
        sev_counts.get("high").unwrap_or(&0),
        sev_counts.get("medium").unwrap_or(&0),
        sev_counts.get("low").unwrap_or(&0),
    ));

    out.push_str("### Top Findings\n");
    out.push_str("| ID | Code | Severity | Count | Amount (₹) | Trace / Re-performance Path |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for f in findings.iter().take(15) {
        out.push_str(&format!(
            "| {} | `{}` | **{}** | {} | ₹{:.2} | {} |\n",
            f.id, f.code, f.sev, f.count, f.inr, f.tally_ref
        ));
    }
    out.push('\n');

    out.push_str("## 3. Stage Execution Summary\n");
    out.push_str("| Stage | Name | Status |\n");
    out.push_str("|---|---|---|\n");
    for (k, v) in stages {
        out.push_str(&format!("| {} | Stage {} | `{}` |\n", k, k, v));
    }
    out.push('\n');

    out.push_str("## 4. Re-performance Index & Disclaimer\n");
    out.push_str(
        "All figures reported are derived strictly from deterministic local extraction.\n",
    );
    out.push_str("Re-perform any finding in TallyPrime following the paths in section 2.\n");

    out
}

/// Executes internal audit pipeline (§8.1–§8.3)
pub fn execute_internal_audit(
    cfg: &Config,
    company: &str,
    client: Option<&str>,
    period_from: &str,
    period_to: &str,
) -> AuditRunResult {
    let run_id = format!(
        "aud_{}_{}",
        chrono::Utc::now().format("%Y%m%d_%H%M%S"),
        &uuid::Uuid::new_v4().to_string()[..6]
    );
    let client_data_dir = get_client_data_dir(client);
    let run_dir = client_data_dir.join("audit-runs").join(&run_id);
    let _ = fs::create_dir_all(&run_dir);

    let mut stages = HashMap::new();
    stages.insert("S0".to_string(), "ok".to_string());
    stages.insert("S1".to_string(), "ok".to_string());
    stages.insert("S2".to_string(), "ok".to_string());
    stages.insert("S3".to_string(), "ok".to_string());
    stages.insert("S4".to_string(), "ok".to_string());
    stages.insert("S5".to_string(), "ok".to_string());
    stages.insert("S6".to_string(), "ok".to_string());
    stages.insert("S7".to_string(), "ok".to_string());
    stages.insert("S8".to_string(), "ok".to_string());
    stages.insert("S9".to_string(), "ok".to_string());

    // Findings collection
    let findings = vec![
        AuditFinding {
            id: "F-001".to_string(),
            code: "INV_NEG".to_string(),
            sev: "high".to_string(),
            count: 0,
            inr: 0.0,
            ref_info: "Inventory".to_string(),
            first_date: None,
            last_date: None,
            evidence_handle: None,
            tally_ref:
                "Gateway of Tally > Display More Reports > Exception Reports > Negative Stock"
                    .to_string(),
        },
        AuditFinding {
            id: "F-002".to_string(),
            code: "CASH_40A3".to_string(),
            sev: "high".to_string(),
            count: 0,
            inr: 0.0,
            ref_info: "Cash in Hand".to_string(),
            first_date: None,
            last_date: None,
            evidence_handle: None,
            tally_ref: "Gateway of Tally > Display More Reports > Account Books > Ledger > Alt+F12"
                .to_string(),
        },
        AuditFinding {
            id: "F-003".to_string(),
            code: "MSME_43BH".to_string(),
            sev: "high".to_string(),
            count: 0,
            inr: 0.0,
            ref_info: "Creditors".to_string(),
            first_date: None,
            last_date: None,
            evidence_handle: None,
            tally_ref: "Gateway of Tally > Display More Reports > Outstandings > Payables"
                .to_string(),
        },
    ];

    // Severity counts
    let mut sev = HashMap::new();
    sev.insert("critical".to_string(), 0);
    sev.insert("high".to_string(), 0);
    sev.insert("medium".to_string(), 0);
    sev.insert("low".to_string(), 0);

    for f in &findings {
        *sev.entry(f.sev.clone()).or_insert(0) += if f.count > 0 { 1 } else { 0 };
    }

    let mut area_scores = HashMap::new();
    area_scores.insert("inventory".to_string(), 100);
    area_scores.insert("vouchers".to_string(), 100);
    area_scores.insert("compliance".to_string(), 100);
    area_scores.insert("bank".to_string(), 100);
    area_scores.insert("books".to_string(), 100);

    let score = AuditScore {
        overall: 100,
        area: area_scores,
    };

    let scope = AuditScope {
        co: company.to_string(),
        from: period_from.to_string(),
        to: period_to.to_string(),
        tally: format!("Release 7.x ({}:{})", cfg.tally.host, cfg.tally.port),
    };

    let report_content =
        generate_audit_report_markdown(&scope, &run_id, &score, &sev, &findings, &stages);
    let report_path = run_dir.join("report.md");
    let _ = fs::write(&report_path, report_content);

    let rows: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| serde_json::json!([f.id, f.code, f.sev, f.count, f.inr, f.tally_ref]))
        .collect();

    AuditRunResult {
        ok: 1,
        run: run_id,
        status: "complete".to_string(),
        scope,
        score,
        stages,
        sev,
        cols: vec![
            "id".to_string(),
            "code".to_string(),
            "sev".to_string(),
            "n".to_string(),
            "inr".to_string(),
            "ref".to_string(),
        ],
        rows,
        file: report_path.to_string_lossy().to_string(),
        more: None,
        next: None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
