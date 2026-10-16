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
}
