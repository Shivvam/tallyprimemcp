use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    NeedsReview,
    PendingApproval,
    Approved,
    Committing,
    Committed,
    CommittedUnverified,
    NeedsReconcile,
    Discarded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherEntry {
    pub ledger: String,
    pub amount: Decimal, // Negative for Debit, Positive for Credit (or vice versa, sums to 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherDraft {
    pub draft_id: String,
    pub company: String,
    pub voucher_type: String, // "Purchase", "Sales", "Journal", "Payment", "Receipt"
    pub date: String,         // YYYYMMDD
    pub number: String,
    pub party_ledger: String,
    pub entries: Vec<VoucherEntry>,
    pub narration: String,
    pub idempotency_key: String,
    pub content_hash: String,
    pub status: DraftStatus,
    #[serde(default)]
    pub tally_guid: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentLine {
    pub description: String,
    pub hsn: Option<String>,
    pub qty: Decimal,
    pub rate: Decimal,
    pub amount: Decimal,
    pub tax_rate: Option<Decimal>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentTax {
    #[serde(rename = "type")]
    pub tax_type: String, // "CGST", "SGST", "IGST"
    pub rate: Decimal,
    pub amount: Decimal,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentExtraction {
    pub doc_type: String,
    pub party_name: String,
    pub party_gstin: Option<String>,
    pub invoice_no: String,
    pub date: String, // YYYY-MM-DD
    pub currency: String,
    pub lines: Vec<DocumentLine>,
    pub taxes: Vec<DocumentTax>,
    pub round_off: Decimal,
    pub total: Decimal,
    #[serde(default)]
    pub confidence: HashMap<String, f64>,
    pub source: DocumentSource,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSource {
    pub file: String,
    pub sha256: String,
}

pub fn compute_idempotency_key(company: &str, vch_type: &str, party: &str, number: &str, date: &str, total: Decimal) -> String {
    use ring::digest::{Context, SHA256};
    let mut context = Context::new(&SHA256);
    let payload = format!("{}|{}|{}|{}|{}|{}", company, vch_type, party, number, date, total);
    context.update(payload.as_bytes());
    hex::encode(context.finish().as_ref())
}

pub fn compute_content_hash(draft: &VoucherDraft) -> String {
    use ring::digest::{Context, SHA256};
    let mut context = Context::new(&SHA256);
    let canonical = serde_json::to_string(draft).unwrap_or_default();
    context.update(canonical.as_bytes());
    hex::encode(context.finish().as_ref())
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
