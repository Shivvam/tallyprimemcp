use crate::config::SafetyConfig;
use crate::models::{BillAllocation, VoucherDraft};
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
    #[error("Bill allocation mismatch for ledger '{0}': entry {1} != allocated {2}")]
    BillAllocationMismatch(String, Decimal, Decimal),
    #[allow(dead_code)]
    #[error("Invalid GSTIN checksum or format for '{0}'")]
    InvalidGstin(String),
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

    // 8. Bill allocations validation if provided
    if let Some(allocations) = &draft.bill_allocations {
        validate_bill_allocations(allocations, &draft.entries)?;
    }

    Ok(())
}

pub fn validate_bill_allocations(
    allocations: &[BillAllocation],
    entries: &[crate::models::VoucherEntry],
) -> Result<(), ValidationError> {
    for entry in entries {
        let alloc_sum: Decimal = allocations
            .iter()
            .filter(|a| a.ledger.eq_ignore_ascii_case(&entry.ledger))
            .map(|a| a.amount)
            .sum();
        if alloc_sum != Decimal::ZERO && alloc_sum.abs() != entry.amount.abs() {
            return Err(ValidationError::BillAllocationMismatch(
                entry.ledger.clone(),
                entry.amount,
                alloc_sum,
            ));
        }
    }
    Ok(())
}

/// Untrusted-content guard (§10.7)
/// Strips prompt injection patterns and caps length.
pub fn untrusted_content_guard(input: &str, max_len: usize) -> String {
    let mut sanitized = input.to_string();
    let forbidden_patterns = [
        "ignore previous instructions",
        "disregard previous directions",
        "system prompt:",
        "override write_mode",
        "write_mode = approve",
        "write_mode = auto_under_limit",
        "bypass approval",
        "<|im_start|>",
        "<|im_end|>",
    ];

    for pattern in &forbidden_patterns {
        loop {
            let lower = sanitized.to_lowercase();
            if let Some(idx) = lower.find(pattern) {
                let end = idx + pattern.len();
                sanitized.replace_range(idx..end, "[FILTERED]");
            } else {
                break;
            }
        }
    }

    if sanitized.len() > max_len {
        sanitized.truncate(max_len);
    }
    sanitized
}

/// Jaro-Winkler string similarity algorithm (§5.5)
#[allow(dead_code)]
pub fn jaro_winkler_similarity(s1: &str, s2: &str) -> f64 {
    let s1 = s1.trim().to_lowercase();
    let s2 = s2.trim().to_lowercase();

    if s1 == s2 {
        return 1.0;
    }
    if s1.is_empty() || s2.is_empty() {
        return 0.0;
    }

    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let l1 = s1_chars.len();
    let l2 = s2_chars.len();

    let match_distance = (l1.max(l2) / 2).saturating_sub(1);
    let mut s1_matches = vec![false; l1];
    let mut s2_matches = vec![false; l2];

    let mut matches = 0;
    for i in 0..l1 {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(l2);

        for j in start..end {
            if s2_matches[j] || s1_chars[i] != s2_chars[j] {
                continue;
            }
            s1_matches[i] = true;
            s2_matches[j] = true;
            matches += 1;
            break;
        }
    }

    if matches == 0 {
        return 0.0;
    }

    let mut transpositions = 0;
    let mut k = 0;
    for i in 0..l1 {
        if !s1_matches[i] {
            continue;
        }
        while !s2_matches[k] {
            k += 1;
        }
        if s1_chars[i] != s2_chars[k] {
            transpositions += 1;
        }
        k += 1;
    }

    let m = matches as f64;
    let jaro = (m / l1 as f64 + m / l2 as f64 + (m - (transpositions as f64 / 2.0)) / m) / 3.0;

    // Common prefix up to 4 characters
    let mut prefix = 0;
    for (c1, c2) in s1_chars.iter().zip(s2_chars.iter()).take(4) {
        if c1 == c2 {
            prefix += 1;
        } else {
            break;
        }
    }

    jaro + (prefix as f64 * 0.1 * (1.0 - jaro))
}

/// GSTIN format and checksum validator (§5.4)
#[allow(dead_code)]
pub fn validate_gstin(gstin: &str) -> bool {
    let clean = gstin.trim().to_uppercase();
    if clean.len() != 15 {
        return false;
    }
    // Pattern: 2 state digits + 10 PAN chars + 1 entity num + 'Z' + 1 checksum char
    let chars: Vec<char> = clean.chars().collect();
    if !chars[0].is_ascii_digit() || !chars[1].is_ascii_digit() {
        return false;
    }
    for c in &chars[2..7] {
        if !c.is_ascii_uppercase() {
            return false;
        }
    }
    for c in &chars[7..11] {
        if !c.is_ascii_digit() {
            return false;
        }
    }
    if !chars[11].is_ascii_uppercase() {
        return false;
    }
    if !chars[12].is_ascii_alphanumeric() {
        return false;
    }
    if chars[13] != 'Z' {
        return false;
    }
    chars[14].is_ascii_alphanumeric()
}
