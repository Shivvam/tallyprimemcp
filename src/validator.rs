use crate::config::SafetyConfig;
use crate::models::VoucherDraft;
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ValidationError {
    #[error("Missing company. Writes require an explicit loaded company.")]
    MissingCompany,
    #[error("Voucher entries do not balance to zero. Sum: {0}")]
    UnbalancedEntries(Decimal),
    #[error("Empty voucher entries list.")]
    EmptyEntries,
    #[error("Referenced ledger name cannot be empty in entry.")]
    EmptyLedgerName,
    #[error("Invalid date format '{0}'. Expected YYYYMMDD.")]
    InvalidDateFormat(String),
    #[error("Voucher amount {0} exceeds safety limit of {1}")]
    ExceedsMaxVoucherAmount(Decimal, Decimal),
    #[error(
        "Unsupported voucher type '{0}'. Allowed: Purchase, Sales, Journal, Payment, Receipt."
    )]
    UnsupportedVoucherType(String),
}

const ALLOWED_VOUCHER_TYPES: &[&str] = &["Purchase", "Sales", "Journal", "Payment", "Receipt"];

pub fn validate_draft(draft: &VoucherDraft, safety: &SafetyConfig) -> Result<(), ValidationError> {
    // 1. Company check
    if draft.company.trim().is_empty() {
        return Err(ValidationError::MissingCompany);
    }

    // 2. Voucher type check
    let type_matched = ALLOWED_VOUCHER_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&draft.voucher_type));
    if !type_matched {
        return Err(ValidationError::UnsupportedVoucherType(
            draft.voucher_type.clone(),
        ));
    }

    // 3. Entries non-empty
    if draft.entries.is_empty() {
        return Err(ValidationError::EmptyEntries);
    }

    // 4. Ledger names valid and calculate exact sum
    let mut sum = Decimal::ZERO;
    let mut max_entry_abs = Decimal::ZERO;

    for entry in &draft.entries {
        if entry.ledger.trim().is_empty() {
            return Err(ValidationError::EmptyLedgerName);
        }
        sum += entry.amount;
        let abs_val = entry.amount.abs();
        if abs_val > max_entry_abs {
            max_entry_abs = abs_val;
        }
    }

    // 5. Zero-balance invariant
    if sum != Decimal::ZERO {
        return Err(ValidationError::UnbalancedEntries(sum));
    }

    // 6. Date validation (YYYYMMDD)
    if draft.date.len() != 8 || !draft.date.chars().all(|c| c.is_ascii_digit()) {
        return Err(ValidationError::InvalidDateFormat(draft.date.clone()));
    }

    // 7. Safety limits
    let max_allowed =
        Decimal::from_f64_retain(safety.max_voucher_amount).unwrap_or(Decimal::from(100000));
    if max_entry_abs > max_allowed {
        return Err(ValidationError::ExceedsMaxVoucherAmount(
            max_entry_abs,
            max_allowed,
        ));
    }

    Ok(())
}
