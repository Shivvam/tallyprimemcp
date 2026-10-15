#[cfg(test)]
mod ui_and_byok_tests {
    use tally_mcp::byok::{is_host_allowed, mock_or_parse_invoice, Document};
    use tally_mcp::config::Config;
    use tally_mcp::journal::JournalManager;
    use tally_mcp::models::{compute_content_hash, DraftStatus, VoucherDraft, VoucherEntry};
    use tally_mcp::ui::CompanionUiServer;
    use rust_decimal::Decimal;

    #[test]
    fn test_egress_allow_list() {
        let extra_hosts = vec!["custom-model.internal".to_string()];

        // Built-in hosts allowed
        assert!(is_host_allowed("api.anthropic.com", &extra_hosts));
        assert!(is_host_allowed("generativelanguage.googleapis.com", &extra_hosts));
        assert!(is_host_allowed("127.0.0.1", &extra_hosts));
        assert!(is_host_allowed("custom-model.internal", &extra_hosts));

        // Malicious or unlisted host strictly blocked
        assert!(!is_host_allowed("evil-exfiltration.com", &extra_hosts));
        assert!(!is_host_allowed("attacker.site", &extra_hosts));
    }

    #[test]
    fn test_document_extraction() {
        let invoice_json = br#"{
            "doc_type": "purchase_invoice",
            "party_name": "Standard Supplies Ltd",
            "party_gstin": "27AAAAA0000A1Z5",
            "invoice_no": "INV-1042",
            "date": "2026-10-12",
            "currency": "INR",
            "lines": [{
                "description": "Component",
                "hsn": "8544",
                "qty": 10,
                "rate": 45,
                "amount": 450,
                "tax_rate": 18
            }],
            "taxes": [{
                "type": "CGST",
                "rate": 9,
                "amount": 40.5
            }],
            "round_off": 0,
            "total": 531,
            "confidence": { "total": 0.99 },
            "source": { "file": "invoice_1042.json", "sha256": "" }
        }"#;

        let doc = Document {
            mime: "application/json".to_string(),
            bytes: invoice_json.to_vec(),
            name: "invoice_1042.json".to_string(),
        };

        let extraction = mock_or_parse_invoice(&doc).expect("Extraction should succeed");
        assert_eq!(extraction.doc_type, "purchase_invoice");
        assert_eq!(extraction.party_name, "Standard Supplies Ltd");
        assert_eq!(extraction.total, Decimal::from(531));
        assert_eq!(extraction.source.file, "invoice_1042.json");
        assert!(!extraction.source.sha256.is_empty());
    }

    #[test]
    fn test_companion_ui_token_and_approval_flow() {
        let config = Config::default();
        let ui_server = CompanionUiServer::new(config);
        assert!(!ui_server.launch_token().is_empty());

        let journal = JournalManager::new().expect("Failed to open journal");
        let draft_id = format!("d_ui_test_{}", uuid::Uuid::new_v4().simple());

        let mut draft = VoucherDraft {
            draft_id: draft_id.clone(),
            company: "Acme Corp".to_string(),
            voucher_type: "Purchase".to_string(),
            date: "20261012".to_string(),
            number: "INV-1042".to_string(),
            party_ledger: "Acme Corp".to_string(),
            entries: vec![
                VoucherEntry { ledger: "Purchases".to_string(), amount: Decimal::from(-100) },
                VoucherEntry { ledger: "Acme Corp".to_string(), amount: Decimal::from(100) },
            ],
            narration: "UI test".to_string(),
            idempotency_key: "idemp_test".to_string(),
            content_hash: String::new(),
            status: DraftStatus::PendingApproval,
            tally_guid: Some("guid-test".to_string()),
        };
        draft.content_hash = compute_content_hash(&draft);
        journal.save_draft(&draft).expect("Failed to save draft");

        // Verify draft loaded
        let loaded = journal.get_draft(&draft_id).expect("Draft should be saved");
        assert_eq!(loaded.status, DraftStatus::PendingApproval);
    }
}
