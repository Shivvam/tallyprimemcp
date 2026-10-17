#[cfg(test)]
mod tests {
    use tally_mcp::tally::client::{
        decode_tally_response, parse_company_names, parse_ledger_names,
    };

    #[test]
    fn test_parse_company_names() {
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <COMPANY>
                            <NAME>Acme Trading Pvt Ltd</NAME>
                        </COMPANY>
                        <COMPANY>
                            <NAME>Global Logistics Ltd</NAME>
                        </COMPANY>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let companies = parse_company_names(sample_xml);
        assert_eq!(companies.len(), 2);
        assert_eq!(companies[0], "Acme Trading Pvt Ltd");
        assert_eq!(companies[1], "Global Logistics Ltd");
    }

    #[test]
    fn test_parse_ledger_names() {
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <LEDGER>
                            <DSPACCNAME>Sales Account</DSPACCNAME>
                        </LEDGER>
                        <LEDGER>
                            <DSPACCNAME>Input CGST</DSPACCNAME>
                        </LEDGER>
                        <LEDGER>
                            <DSPACCNAME>Acme Corp</DSPACCNAME>
                        </LEDGER>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let ledgers = parse_ledger_names(sample_xml);
        assert_eq!(ledgers.len(), 3);
        assert_eq!(ledgers[0], "Sales Account");
        assert_eq!(ledgers[1], "Input CGST");
        assert_eq!(ledgers[2], "Acme Corp");
    }

    #[test]
    fn test_utf16_bom_decode() {
        // UTF-16LE BOM: FF FE followed by 'T' (0x54 0x00), 'a' (0x61 0x00), 'l' (0x6C 0x00)
        let bytes = vec![0xFF, 0xFE, 0x54, 0x00, 0x61, 0x00, 0x6C, 0x00];
        let decoded = decode_tally_response(&bytes);
        assert_eq!(decoded, "Tal");
    }

    #[test]
    fn test_parse_company_names_cdata_and_attributes() {
        // Test CDATA wrapping and COMPANY attributes (common in production TallyPrime XML)
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <COMPANY NAME="Attribute Company Pvt Ltd">
                            <NAME><![CDATA[CDATA Company Ltd]]></NAME>
                        </COMPANY>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let companies = parse_company_names(sample_xml);
        assert!(companies.contains(&"Attribute Company Pvt Ltd".to_string()));
        assert!(companies.contains(&"CDATA Company Ltd".to_string()));
    }

    #[test]
    fn test_parse_company_names_svcurrentcompany() {
        // When only header/staticvariables contains the current active company
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DESC>
                    <STATICVARIABLES>
                        <SVCURRENTCOMPANY>Single Active Enterprise</SVCURRENTCOMPANY>
                    </STATICVARIABLES>
                </DESC>
                <DATA>
                    <COLLECTION></COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let companies = parse_company_names(sample_xml);
        assert_eq!(companies, vec!["Single Active Enterprise"]);
    }

    #[test]
    fn test_parse_ledger_names_cdata_and_attributes() {
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <LEDGER NAME="HDFC Bank Current A/c">
                            <DSPACCNAME><![CDATA[State Bank of India]]></DSPACCNAME>
                        </LEDGER>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let ledgers = parse_ledger_names(sample_xml);
        assert!(ledgers.contains(&"HDFC Bank Current A/c".to_string()));
        assert!(ledgers.contains(&"State Bank of India".to_string()));
    }

    #[test]
    fn test_dechunk_http_body() {
        use tally_mcp::tally::client::dechunk_http_body;

        // Chunk 1: size 5 (hello), Chunk 2: size 6 ( world), Terminal chunk: 0
        let raw = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let dechunked = dechunk_http_body(raw);
        assert_eq!(String::from_utf8_lossy(&dechunked), "hello world");
    }

    #[tokio::test]
    async fn test_status_when_offline() {
        use tally_mcp::config::TallyConfig;
        use tally_mcp::tally::client::TallyClient;

        let config = TallyConfig {
            host: "127.0.0.1".to_string(),
            port: 1, // unreachable port
            ..Default::default()
        };

        let client = TallyClient::new(config);
        let status = client
            .status()
            .await
            .expect("Status call should succeed even when offline");
        assert!(
            !status.reachable,
            "Should cleanly report unreachable without error panic"
        );
        assert!(status.loaded_companies.is_empty());
        assert!(status.error.is_some(), "Should contain error description");
    }

    #[test]
    fn test_parse_company_names_apna_mart_format() {
        // Exact format reported from TallyPrime via PowerShell query
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <COMPANY NAME="Apna Mart" RESERVEDNAME="">
                            <NAME TYPE="String">Apna Mart</NAME>
                        </COMPANY>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let companies = parse_company_names(sample_xml);
        assert_eq!(companies, vec!["Apna Mart".to_string()]);
    }

    #[test]
    fn test_parse_company_names_self_closing_tag() {
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <COMPANY NAME="Self Closing Traders Ltd" RESERVEDNAME=""/>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let companies = parse_company_names(sample_xml);
        assert_eq!(companies, vec!["Self Closing Traders Ltd".to_string()]);
    }

    #[test]
    fn test_parse_ledger_names_filters_out_groups_and_guids() {
        // Tally List of Accounts returns groups, ledgers, and remote company GUIDs
        let sample_xml = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <GROUP NAME="Capital Account" RESERVEDNAME="">
                            <NAME TYPE="String">Capital Account</NAME>
                        </GROUP>
                        <GROUP NAME="Sundry Debtors" RESERVEDNAME="">
                            <NAME TYPE="String">Sundry Debtors</NAME>
                        </GROUP>
                        <REMOTECMPINFO.LIST>
                            <NAME TYPE="String">407d9af7-f23c-4983-801d-c165e2d1f700</NAME>
                        </REMOTECMPINFO.LIST>
                        <LEDGER NAME="Cash" RESERVEDNAME="">
                            <NAME TYPE="String">Cash</NAME>
                            <PARENT>Cash-in-Hand</PARENT>
                        </LEDGER>
                        <LEDGER NAME="M/s Sr Traders" RESERVEDNAME="">
                            <NAME TYPE="String">M/s Sr Traders</NAME>
                            <PARENT>Sundry Debtors</PARENT>
                        </LEDGER>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let ledgers = parse_ledger_names(sample_xml);
        // Groups must be excluded
        assert!(!ledgers.contains(&"Capital Account".to_string()));
        assert!(!ledgers.contains(&"Sundry Debtors".to_string()));
        // Company GUID must be excluded
        assert!(!ledgers.contains(&"407d9af7-f23c-4983-801d-c165e2d1f700".to_string()));
        // Real ledgers must be present
        assert!(ledgers.contains(&"Cash".to_string()));
        assert!(ledgers.contains(&"M/s Sr Traders".to_string()));
        assert_eq!(ledgers.len(), 2);
    }

    #[test]
    fn test_extract_tally_lineerror() {
        use tally_mcp::tally::client::extract_tally_error;

        let err_xml = r#"<RESPONSE>
            <STATUS>0</STATUS>
            <LINEERROR>Could not find Report 'Bills Outstanding'</LINEERROR>
        </RESPONSE>"#;

        let err = extract_tally_error(err_xml);
        assert_eq!(
            err,
            Some("Tally LINEERROR: Could not find Report 'Bills Outstanding'".to_string())
        );

        let clean_xml = "<ENVELOPE><BODY><DATA></DATA></BODY></ENVELOPE>";
        assert!(extract_tally_error(clean_xml).is_none());
    }

    #[test]
    fn test_empty_envelope_and_format_report() {
        use tally_mcp::tally::client::{format_report_response, is_empty_envelope};

        assert!(is_empty_envelope("<ENVELOPE></ENVELOPE>"));
        assert!(is_empty_envelope("<ENVELOPE/>"));
        assert!(is_empty_envelope("  <ENVELOPE>\n  </ENVELOPE>  "));

        let msg = format_report_response("Trial Balance", "<ENVELOPE></ENVELOPE>");
        assert!(msg.contains("No data records found in Tally for 'Trial Balance'"));
    }

    #[test]
    fn test_parse_day_book_summary() {
        use tally_mcp::tally::client::format_day_book_response;

        let sample_day_book = r#"<ENVELOPE>
            <BODY>
                <DATA>
                    <COLLECTION>
                        <VOUCHER DATE="20260401" VCHTYPE="Receipt">
                            <DATE>20260401</DATE>
                            <VOUCHERTYPENAME>Receipt</VOUCHERTYPENAME>
                            <VOUCHERNUMBER>1</VOUCHERNUMBER>
                            <PARTYLEDGERNAME>Cash</PARTYLEDGERNAME>
                            <AMOUNT>-5000.00</AMOUNT>
                            <ISCANCELLED>Yes</ISCANCELLED>
                            <NARRATION>Opening receipt</NARRATION>
                            <ALLLEDGERENTRIES.LIST>
                                <LEDGERNAME>Cash</LEDGERNAME>
                                <AMOUNT>-5000.00</AMOUNT>
                            </ALLLEDGERENTRIES.LIST>
                        </VOUCHER>
                    </COLLECTION>
                </DATA>
            </BODY>
        </ENVELOPE>"#;

        let formatted = format_day_book_response(sample_day_book);
        assert!(formatted.contains("\"voucher_type\": \"Receipt\""));
        assert!(formatted.contains("\"voucher_number\": \"1\""));
        assert!(formatted.contains("\"date\": \"2026-04-01\""));
        assert!(formatted.contains("\"is_cancelled\": true"));
        assert!(formatted.contains("\"party\": \"Cash\""));
    }
}
