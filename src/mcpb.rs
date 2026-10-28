//! Module for generating Claude Desktop extension bundle (.mcpb).
//! Claude Desktop extensions are standard PKZip archives containing a `manifest.json`.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const TOOLS_META_JSON: &str = include_str!("../tools/meta.json");

/// Creates an uncompressed ZIP archive containing a single file in memory.
/// This complies with standard PKZip specification without requiring external tools or crates.
pub fn create_stored_zip(filename: &str, data: &[u8]) -> Vec<u8> {
    let name_bytes = filename.as_bytes();
    let name_len = name_bytes.len() as u16;
    let size = data.len() as u32;

    // Standard IEEE 802.3 CRC-32
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if (crc & 1) != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    let crc = !crc;

    let mut zip = Vec::with_capacity(data.len() + 256);

    // 1. Local File Header (30 bytes + name_len + data.len())
    zip.extend_from_slice(b"PK\x03\x04");
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // method: 0 = stored
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod time
    zip.extend_from_slice(&0x5821u16.to_le_bytes()); // mod date: 2024-01-01
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes()); // compressed size
    zip.extend_from_slice(&size.to_le_bytes()); // uncompressed size
    zip.extend_from_slice(&name_len.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra field len
    zip.extend_from_slice(name_bytes);
    zip.extend_from_slice(data);

    let cd_offset = zip.len() as u32;

    // 2. Central Directory File Header (46 bytes + name_len)
    zip.extend_from_slice(b"PK\x01\x02");
    zip.extend_from_slice(&20u16.to_le_bytes()); // version made by: 2.0
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // method: 0 = stored
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod time
    zip.extend_from_slice(&0x5821u16.to_le_bytes()); // mod date: 2024-01-01
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes()); // compressed size
    zip.extend_from_slice(&size.to_le_bytes()); // uncompressed size
    zip.extend_from_slice(&name_len.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra field len
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment len
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk # start
    zip.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    zip.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    zip.extend_from_slice(&0u32.to_le_bytes()); // local header offset: 0
    zip.extend_from_slice(name_bytes);

    let cd_size = (zip.len() as u32) - cd_offset;

    // 3. End of Central Directory Record (22 bytes)
    zip.extend_from_slice(b"PK\x05\x06");
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk number: 0
    zip.extend_from_slice(&0u16.to_le_bytes()); // start disk: 0
    zip.extend_from_slice(&1u16.to_le_bytes()); // records on this disk: 1
    zip.extend_from_slice(&1u16.to_le_bytes()); // total records: 1
    zip.extend_from_slice(&cd_size.to_le_bytes());
    zip.extend_from_slice(&cd_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment len: 0

    zip
}

/// Generates the manifest.json content for Claude Desktop MCP extension.
pub fn generate_manifest(host: Option<&str>, port: Option<u16>) -> serde_json::Value {
    let host_val = host.unwrap_or("127.0.0.1");
    let port_val = port.unwrap_or(9000);

    let mut serve_args = vec!["serve".to_string()];
    if host_val != "127.0.0.1" && host_val != "localhost" {
        serve_args.push("--host".to_string());
        serve_args.push(host_val.to_string());
    }
    if port_val != 9000 {
        serve_args.push("--port".to_string());
        serve_args.push(port_val.to_string());
    }

    let meta_tools: Vec<serde_json::Value> =
        serde_json::from_str(TOOLS_META_JSON).unwrap_or_default();
    let tools: Vec<String> = meta_tools
        .iter()
        .filter_map(|t| {
            t.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    json!({
        "dxt_version": "0.1",
        "name": "tally-prime",
        "display_name": "Tally Prime",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "TallyMCP: Connect Claude Desktop to TallyPrime locally over port 9000 with human approval gating all writes.",
        "author": {
            "name": "Shivvam",
            "url": "https://github.com/Shivvam/tallyprimemcp"
        },
        "homepage": "https://www.tallymcp.pro/",
        "repository": "https://github.com/Shivvam/tallyprimemcp",
        "server": {
            "type": "binary",
            "mcp_config": {
                "command": "tally-mcp",
                "args": serve_args,
                "env": {
                    "TALLY_HOST": host_val,
                    "TALLY_PORT": port_val.to_string()
                }
            }
        },
        "tools": tools
    })
}

/// Builds the .mcpb bundle bytes in memory.
pub fn create_bundle_bytes(host: Option<&str>, port: Option<u16>) -> Vec<u8> {
    let manifest = generate_manifest(host, port);
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap_or_default();
    create_stored_zip("manifest.json", &manifest_bytes)
}

/// Exports the .mcpb bundle to the target file path.
/// If target_path is None, defaults to `tally-mcp-<version>.mcpb` in the current working directory.
pub fn export_bundle_to_file(
    target_path: Option<&Path>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let default_name = format!("tally-mcp-{}.mcpb", env!("CARGO_PKG_VERSION"));
    let path = match target_path {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(default_name),
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let bundle_bytes = create_bundle_bytes(None, None);
    fs::write(&path, &bundle_bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_generation() {
        let manifest = generate_manifest(None, None);
        assert_eq!(manifest["dxt_version"], "0.1");
        assert_eq!(manifest["name"], "tally-prime");
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        let tools = manifest["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 35);
        assert!(tools.iter().any(|t| t == "audit_sec_269ss_loan_acceptance"));
        assert!(tools.iter().any(|t| t == "audit_sec_269t_loan_repayment"));
        assert!(tools.iter().any(|t| t == "audit_sec_40a3_cash_expenditure"));
        assert!(tools.iter().any(|t| t == "audit_labour_pf_esi_36_1_va"));
    }

    #[test]
    fn test_create_bundle_and_verify_zip() {
        let zip_bytes = create_bundle_bytes(None, None);
        assert!(zip_bytes.starts_with(b"PK\x03\x04"));

        // Extract manifest.json from local file header
        // Header is 30 bytes + name_len (13 bytes for "manifest.json")
        let header_len = 30 + 13;
        assert!(zip_bytes.len() > header_len);
        let filename = &zip_bytes[30..43];
        assert_eq!(filename, b"manifest.json");

        // Parse manifest from zip payload
        // The payload starts at index 43 and goes until Central Directory (PK\x01\x02)
        let cd_pos = zip_bytes
            .windows(4)
            .position(|w| w == b"PK\x01\x02")
            .expect("Central directory header");
        let payload = &zip_bytes[43..cd_pos];
        let parsed: serde_json::Value =
            serde_json::from_slice(payload).expect("Valid JSON in manifest");
        assert_eq!(parsed["name"], "tally-prime");
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
    }
}
