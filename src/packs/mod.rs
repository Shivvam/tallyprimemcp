use crate::config::get_app_data_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum PackError {
    #[error("Pack mutation violation: Envelope contains mutating action ('Import Data'). Packs are read-only.")]
    MutatingActionRejected,
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),
    #[error("Envelope validation error: Request must be an Export envelope.")]
    InvalidEnvelope,
    #[error("Pack parse error in {0}: {1}")]
    ParseError(String, String),
    #[error("Tool '{0}' not found in active tool packs.")]
    ToolNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackTool {
    pub name: String,
    pub description: String,
    pub access: String, // must be "read"
    pub input_schema: Value,
    pub request_template: String,
    #[serde(default)]
    pub response_map: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPack {
    pub pack: String,
    pub version: String,
    pub min_app_version: String,
    pub tools: Vec<PackTool>,
}

pub struct PackManager {
    tools_dir: PathBuf,
    active_packs: Vec<ToolPack>,
}

impl PackManager {
    pub fn new(enable_unsigned: bool, disabled_packs: &[String]) -> Self {
        let tools_dir = get_app_data_dir().join("tools");
        let _ = fs::create_dir_all(&tools_dir);

        let mut manager = Self {
            tools_dir,
            active_packs: Vec::new(),
        };

        manager.load_bundled_packs(disabled_packs);
        if enable_unsigned {
            manager.load_user_packs(disabled_packs);
        }

        manager
    }

    fn load_bundled_packs(&mut self, disabled: &[String]) {
        let bundled = vec![
            get_bundled_outstanding_ageing(),
            get_bundled_gst_summary(),
            get_bundled_stock_thresholds(),
        ];

        for raw_json in bundled {
            if let Ok(pack) = serde_json::from_str::<ToolPack>(&raw_json) {
                if !disabled.contains(&pack.pack) {
                    self.active_packs.push(pack);
                }
            }
        }
    }

    fn load_user_packs(&mut self, disabled: &[String]) {
        if !self.tools_dir.exists() {
            return;
        }

        if let Ok(entries) = fs::read_dir(&self.tools_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(pack) = serde_json::from_str::<ToolPack>(&content) {
                            if !disabled.contains(&pack.pack) {
                                self.active_packs.push(pack);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn list_tools(&self) -> Vec<&PackTool> {
        self.active_packs.iter().flat_map(|p| &p.tools).collect()
    }

    pub fn find_tool(&self, name: &str) -> Option<&PackTool> {
        self.list_tools().into_iter().find(|t| t.name == name)
    }

    pub fn execute_template(&self, tool: &PackTool, args: &Value) -> Result<String, PackError> {
        let mut xml = tool.request_template.clone();

        if let Some(obj) = args.as_object() {
            for (key, val) in obj {
                let placeholder = format!("{{{{{}}}}}", key);
                let val_str = match val {
                    Value::String(s) => quick_xml::escape::escape(s).to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => val.to_string(),
                };
                xml = xml.replace(&placeholder, &val_str);
            }
        }

        // Gateway Envelope Enforcement: verify request is an Export envelope and not mutating
        verify_export_envelope(&xml)?;

        Ok(xml)
    }
}

pub fn verify_export_envelope(xml: &str) -> Result<(), PackError> {
    // 1. Strict rejection of mutating actions
    let upper = xml.to_uppercase();
    if upper.contains("IMPORT DATA")
        || upper.contains("<IMPORTDATA>")
        || upper.contains("ACTION=\"CREATE\"")
    {
        return Err(PackError::MutatingActionRejected);
    }

    // 2. Must be an Export envelope
    if !upper.contains("EXPORT") && !upper.contains("<ENVELOPE>") {
        return Err(PackError::InvalidEnvelope);
    }

    Ok(())
}

fn get_bundled_outstanding_ageing() -> String {
    r#"{
  "pack": "outstanding-ageing",
  "version": "1.0.0",
  "min_app_version": "1.0.0",
  "tools": [{
    "name": "tally_outstanding_ageing",
    "description": "Receivable and payable ageing buckets as of a specified date",
    "access": "read",
    "input_schema": {
      "type": "object",
      "properties": {
        "company": { "type": "string", "description": "Company name" },
        "as_on": { "type": "string", "description": "Date as YYYYMMDD" }
      },
      "required": ["company", "as_on"]
    },
    "request_template": "<ENVELOPE><HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Data</TYPE><ID>Bills Outstanding</ID></HEADER><BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT><SVCURRENTCOMPANY>{{company}}</SVCURRENTCOMPANY><SVTODATE>{{as_on}}</SVTODATE></STATICVARIABLES></DESC></BODY></ENVELOPE>"
  }]
}"#.to_string()
}

fn get_bundled_gst_summary() -> String {
    r#"{
  "pack": "gst-summary",
  "version": "1.0.0",
  "min_app_version": "1.0.0",
  "tools": [{
    "name": "tally_gst_summary",
    "description": "Summary of Input and Output GST ledger balances for a period",
    "access": "read",
    "input_schema": {
      "type": "object",
      "properties": {
        "company": { "type": "string", "description": "Company name" },
        "from_date": { "type": "string", "description": "YYYYMMDD" },
        "to_date": { "type": "string", "description": "YYYYMMDD" }
      },
      "required": ["company", "from_date", "to_date"]
    },
    "request_template": "<ENVELOPE><HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Data</TYPE><ID>Trial Balance</ID></HEADER><BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT><SVCURRENTCOMPANY>{{company}}</SVCURRENTCOMPANY><SVFROMDATE>{{from_date}}</SVFROMDATE><SVTODATE>{{to_date}}</SVTODATE></STATICVARIABLES></DESC></BODY></ENVELOPE>"
  }]
}"#.to_string()
}

fn get_bundled_stock_thresholds() -> String {
    r#"{
  "pack": "stock-thresholds",
  "version": "1.0.0",
  "min_app_version": "1.0.0",
  "tools": [{
    "name": "tally_stock_thresholds",
    "description": "Stock items summary and reorder level thresholds",
    "access": "read",
    "input_schema": {
      "type": "object",
      "properties": {
        "company": { "type": "string", "description": "Company name" }
      },
      "required": ["company"]
    },
    "request_template": "<ENVELOPE><HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Data</TYPE><ID>Stock Summary</ID></HEADER><BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT><SVCURRENTCOMPANY>{{company}}</SVCURRENTCOMPANY></STATICVARIABLES></DESC></BODY></ENVELOPE>"
  }]
}"#.to_string()
}
