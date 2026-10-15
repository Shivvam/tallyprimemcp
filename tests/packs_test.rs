#[cfg(test)]
mod packs_tests {
    use serde_json::json;
    use tally_mcp::packs::{verify_export_envelope, PackError, PackManager};

    #[test]
    fn test_bundled_packs_loaded() {
        let manager = PackManager::new(false, &[]);
        let tools = manager.list_tools();

        let tool_names: Vec<String> = tools.into_iter().map(|t| t.name.clone()).collect();
        assert!(tool_names.contains(&"tally_outstanding_ageing".to_string()));
        assert!(tool_names.contains(&"tally_gst_summary".to_string()));
        assert!(tool_names.contains(&"tally_stock_thresholds".to_string()));
    }

    #[test]
    fn test_template_expansion_with_escaping() {
        let manager = PackManager::new(false, &[]);
        let tool = manager
            .find_tool("tally_outstanding_ageing")
            .expect("outstanding ageing tool should exist");

        let args = json!({
            "company": "Acme & Co. <Pvt> Ltd",
            "as_on": "20261015"
        });

        let expanded = manager.execute_template(tool, &args).expect("Expansion should succeed");
        assert!(expanded.contains("&lt;Pvt&gt;"));
        assert!(expanded.contains("<SVTODATE>20261015</SVTODATE>"));
        assert!(expanded.contains("<TALLYREQUEST>Export</TALLYREQUEST>"));
    }

    #[test]
    fn test_gateway_envelope_enforcement_blocks_mutation() {
        // Attack simulation: Pack author attempts to inject an Import Data envelope
        let malicious_xml = "<ENVELOPE><HEADER><TALLYREQUEST>Import Data</TALLYREQUEST></HEADER><BODY><IMPORTDATA>malicious payload</IMPORTDATA></BODY></ENVELOPE>";
        let res = verify_export_envelope(malicious_xml);
        assert!(matches!(res, Err(PackError::MutatingActionRejected)));

        // Action=Create injection attack
        let malicious_action_xml = "<ENVELOPE><HEADER><TALLYREQUEST>Export</TALLYREQUEST></HEADER><BODY ACTION=\"Create\"></BODY></ENVELOPE>";
        let res2 = verify_export_envelope(malicious_action_xml);
        assert!(matches!(res2, Err(PackError::MutatingActionRejected)));
    }

    #[test]
    fn test_valid_export_envelope_accepted() {
        let valid_export_xml = "<ENVELOPE><HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Data</TYPE></HEADER><BODY><DESC></DESC></BODY></ENVELOPE>";
        let res = verify_export_envelope(valid_export_xml);
        assert!(res.is_ok());
    }
}
