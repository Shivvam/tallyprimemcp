#[cfg(test)]
mod write_safety_tests {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use tally_mcp::config::SafetyConfig;
    use tally_mcp::journal::JournalManager;
    use tally_mcp::models::{
        compute_content_hash, compute_idempotency_key, DraftStatus, VoucherDraft, VoucherEntry,
    };
    use tally_mcp::tally::builder::{build_voucher_import_xml, parse_import_response};
    use tally_mcp::validator::{validate_draft, ValidationError};

    fn make_valid_draft() -> VoucherDraft {
        let entries = vec![
            VoucherEntry {
                ledger: "Hardware Purchases".to_string(),
                amount: Decimal::from_str("-450.00").unwrap(),
            },
            VoucherEntry {
                ledger: "Input CGST".to_string(),
                amount: Decimal::from_str("-40.50").unwrap(),
            },
            VoucherEntry {
                ledger: "Input SGST".to_string(),
                amount: Decimal::from_str("-40.50").unwrap(),
            },
            VoucherEntry {
                ledger: "Acme Corp".to_string(),
                amount: Decimal::from_str("531.00").unwrap(),
            },
        ];

        let mut draft = VoucherDraft {
            draft_id: "d_test_001".to_string(),
            company: "Acme Trading Pvt Ltd".to_string(),
            voucher_type: "Purchase".to_string(),
            date: "20261012".to_string(),
            number: "INV-1042".to_string(),
            party_ledger: "Acme Corp".to_string(),
            entries,
            narration: "Test voucher".to_string(),
            idempotency_key: String::new(),
            content_hash: String::new(),
            status: DraftStatus::NeedsReview,
            tally_guid: Some("guid-test-123".to_string()),
            ..Default::default()
        };

        draft.idempotency_key = compute_idempotency_key(
            &draft.company,
            &draft.voucher_type,
            &draft.party_ledger,
            &draft.number,
            &draft.date,
            Decimal::from_str("531.00").unwrap(),
        );
        draft.content_hash = compute_content_hash(&draft);
        draft
    }

    #[test]
    fn test_valid_draft_balances_to_zero() {
        let draft = make_valid_draft();
        let safety = SafetyConfig::default();
        let res = validate_draft(&draft, &safety);
        assert!(res.is_ok());
    }

    #[test]
    fn test_unbalanced_draft_rejected() {
        let mut draft = make_valid_draft();
        draft.entries[3].amount = Decimal::from_str("530.00").unwrap(); // 1 rupee discrepancy
        let safety = SafetyConfig::default();
        let res = validate_draft(&draft, &safety);
        assert!(matches!(res, Err(ValidationError::UnbalancedEntries(_))));
    }

    #[test]
    fn test_deterministic_xml_builder() {
        let draft = make_valid_draft();
        let xml = build_voucher_import_xml(&draft);

        assert!(xml.contains("<TALLYREQUEST>Import Data</TALLYREQUEST>"));
        assert!(xml.contains("<SVCURRENTCOMPANY>Acme Trading Pvt Ltd</SVCURRENTCOMPANY>"));
        assert!(xml.contains(r#"<VOUCHER VCHTYPE="Purchase" ACTION="Create">"#));
        assert!(xml.contains("<GUID>guid-test-123</GUID>"));
        assert!(xml.contains("<DATE>20261012</DATE>"));
        assert!(xml.contains("<EFFECTIVEDATE>20261012</EFFECTIVEDATE>"));
        assert!(xml.contains("<VOUCHERNUMBER>INV-1042</VOUCHERNUMBER>"));
        assert!(xml.contains("<LEDGERNAME>Acme Corp</LEDGERNAME>"));
        assert!(xml.contains("<AMOUNT>531.00</AMOUNT>"));
    }

    #[test]
    fn test_parse_import_response_success() {
        let sample_resp = r#"<RESPONSE>
            <CREATED>1</CREATED>
            <ALTERED>0</ALTERED>
            <ERRORS>0</ERRORS>
        </RESPONSE>"#;

        let parsed = parse_import_response(sample_resp);
        assert_eq!(parsed.created, 1);
        assert_eq!(parsed.altered, 0);
        assert_eq!(parsed.errors, 0);
        assert!(parsed.error_message.is_none());
    }

    #[test]
    fn test_parse_import_response_error() {
        let sample_resp = r#"<RESPONSE>
            <CREATED>0</CREATED>
            <ALTERED>0</ALTERED>
            <ERRORS>1</ERRORS>
            <LINEERROR>Ledger 'Unknown Ledger' does not exist</LINEERROR>
        </RESPONSE>"#;

        let parsed = parse_import_response(sample_resp);
        assert_eq!(parsed.created, 0);
        assert_eq!(parsed.errors, 1);
        assert_eq!(
            parsed.error_message,
            Some("Ledger 'Unknown Ledger' does not exist".to_string())
        );
    }

    #[test]
    fn test_parse_import_response_exceptions_counter_not_treated_as_error_msg() {
        let sample_resp = r#"<RESPONSE>
            <STATUS>1</STATUS>
            <CREATED>0</CREATED>
            <ALTERED>0</ALTERED>
            <ERRORS>1</ERRORS>
            <EXCEPTIONS>1</EXCEPTIONS>
        </RESPONSE>"#;

        let parsed = parse_import_response(sample_resp);
        assert_eq!(parsed.created, 0);
        assert_eq!(parsed.errors, 1);
        // Error message must not be literal "1"
        assert_ne!(parsed.error_message, Some("1".to_string()));
        assert!(parsed.error_message.unwrap().contains("ERRORS: 1"));
    }

    #[test]
    fn test_journal_crash_reconciliation() {
        let manager = JournalManager::new().expect("Failed to create journal manager");
        let mut draft = make_valid_draft();
        draft.draft_id = format!("d_crash_{}", uuid::Uuid::new_v4().simple());
        draft.status = DraftStatus::Committing;

        manager.save_draft(&draft).expect("Failed to save draft");

        // Execute startup reconciliation
        let reconciled = manager.reconcile_startup().expect("Reconciliation failed");
        assert!(reconciled.contains(&draft.draft_id));

        // Verify draft was safely reverted to Approved (never lost, never re-sent blindly)
        let loaded = manager
            .get_draft(&draft.draft_id)
            .expect("Draft should exist");
        assert_eq!(loaded.status, DraftStatus::Approved);
    }
}
