use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    #[default]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DraftOrigin {
    #[default]
    Manual,
    Document,
    Bank,
    Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherEntry {
    pub ledger: String,
    pub amount: Decimal, // Negative for Debit, Positive for Credit
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillAllocation {
    pub ledger: String,
    pub ref_name: String,
    pub kind: String, // "new" | "against" | "advance"
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostCentreAllocation {
    pub ledger: String,
    pub centre: String,
    pub amount: Decimal,
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
    #[serde(default)]
    pub origin: DraftOrigin,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub reverses: Option<String>,
    #[serde(default)]
    pub bill_allocations: Option<Vec<BillAllocation>>,
    #[serde(default)]
    pub cost_centres: Option<Vec<CostCentreAllocation>>,
    #[serde(default)]
    pub approved_by: Option<String>,
}

impl Default for VoucherDraft {
    fn default() -> Self {
        Self {
            draft_id: String::new(),
            company: String::new(),
            voucher_type: String::new(),
            date: String::new(),
            number: String::new(),
            party_ledger: String::new(),
            entries: Vec::new(),
            narration: String::new(),
            idempotency_key: String::new(),
            content_hash: String::new(),
            status: DraftStatus::NeedsReview,
            tally_guid: None,
            origin: DraftOrigin::Manual,
            warnings: Vec::new(),
            reverses: None,
            bill_allocations: None,
            cost_centres: None,
            approved_by: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceLine {
    pub item_or_ledger: String,
    pub qty: Option<Decimal>,
    pub rate: Option<Decimal>,
    pub amount: Decimal,
    pub hsn: Option<String>,
    pub gst_rate: Option<Decimal>,
    pub godown: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceDraft {
    pub draft_id: String,
    pub invoice_type: String, // "sales", "purchase", "credit_note", "debit_note"
    pub date: String,
    pub number: Option<String>,
    pub party: String,
    pub lines: Vec<InvoiceLine>,
    pub place_of_supply: String,
    pub ref_invoice: Option<String>,
    pub narration: Option<String>,
    pub source_handle: Option<String>,
    pub status: DraftStatus,
    pub origin: DraftOrigin,
    pub warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterDraft {
    pub draft_id: String,
    pub kind: String, // "ledger", "party", "stock_item", "group"
    pub name: String,
    pub parent_group: String,
    pub gstin: Option<String>,
    pub pan: Option<String>,
    pub state: Option<String>,
    pub unit: Option<String>,
    pub hsn: Option<String>,
    pub gst_rate: Option<Decimal>,
    pub valuation_method: Option<String>,
    pub confirm_not_duplicate: bool,
    pub status: DraftStatus,
    pub warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankRule {
    pub pattern: String,
    pub ledger: String,
    pub voucher_type: String,
    pub created_from_draft_id: Option<String>,
    pub approved_by: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub ok: u8,
    pub cols: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tot: Option<serde_json::Value>,
    pub n: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub ok: u8,
    pub err: String,
    pub msg: String,
    pub fix: String,
}

impl ErrorResponse {
    pub fn new(err: &str, msg: &str, fix: &str) -> Self {
        Self {
            ok: 0,
            err: err.to_string(),
            msg: msg.to_string(),
            fix: fix.to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
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

pub fn compute_idempotency_key(
    company: &str,
    vch_type: &str,
    party: &str,
    number: &str,
    date: &str,
    total: Decimal,
) -> String {
    use ring::digest::{Context, SHA256};
    let mut context = Context::new(&SHA256);
    let payload = format!(
        "{}|{}|{}|{}|{}|{}",
        company, vch_type, party, number, date, total
    );
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
