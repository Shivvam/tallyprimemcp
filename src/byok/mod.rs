#![allow(dead_code)]
use crate::models::{DocumentExtraction, DocumentSource};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExtractError {
    #[error("Egress rejected: Host '{0}' is not in the egress allow-list.")]
    HostNotAllowed(String),
    #[error("API Key not found for provider '{0}'. Configure it via setup or UI.")]
    MissingApiKey(String),
    #[error("Provider '{0}' request failed: {1}")]
    RequestFailed(String, String),
    #[error("Failed to parse provider response: {0}")]
    ParseError(String),
    #[error("MIME type '{0}' not supported for extraction.")]
    UnsupportedMime(String),
}

#[derive(Debug, Clone)]
pub struct Document {
    pub mime: String,
    pub bytes: Vec<u8>,
    pub name: String,
}

pub trait Extractor: Send + Sync {
    fn id(&self) -> &'static str;
    fn extract(&self, doc: &Document) -> Result<DocumentExtraction, ExtractError>;
}

// Built-in compiled cloud provider hosts
pub const BUILTIN_ALLOWED_HOSTS: &[&str] = &[
    "api.anthropic.com",
    "generativelanguage.googleapis.com",
    "api.openai.com",
    "127.0.0.1",
    "localhost",
];

pub fn is_host_allowed(host: &str, extra_hosts: &[String]) -> bool {
    let host_clean = host.split(':').next().unwrap_or(host);
    if BUILTIN_ALLOWED_HOSTS.contains(&host_clean) {
        return true;
    }
    extra_hosts.iter().any(|h| h.split(':').next().unwrap_or(h) == host_clean)
}

pub fn get_api_key(provider: &str) -> Option<String> {
    // 1. Try environment variable (e.g., ANTHROPIC_API_KEY, GEMINI_API_KEY, OPENAI_API_KEY)
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "AI_API_KEY",
    };

    if let Ok(val) = std::env::var(env_var) {
        if !val.trim().is_empty() {
            return Some(val);
        }
    }

    // 2. Try OS Credential Manager via keyring
    #[cfg(feature = "byok")]
    {
        if let Ok(entry) = keyring::Entry::new("TallyMCP", provider) {
            if let Ok(secret) = entry.get_password() {
                if !secret.trim().is_empty() {
                    return Some(secret);
                }
            }
        }
    }

    None
}

pub fn set_api_key(provider: &str, key: &str) -> Result<(), String> {
    #[cfg(feature = "byok")]
    {
        let entry = keyring::Entry::new("TallyMCP", provider).map_err(|e| e.to_string())?;
        entry.set_password(key).map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(feature = "byok"))]
    {
        let _ = (provider, key);
        Err("byok feature not enabled".to_string())
    }
}

pub struct AnthropicExtractor {
    pub extra_hosts: Vec<String>,
}

impl Extractor for AnthropicExtractor {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn extract(&self, doc: &Document) -> Result<DocumentExtraction, ExtractError> {
        let host = "api.anthropic.com";
        if !is_host_allowed(host, &self.extra_hosts) {
            return Err(ExtractError::HostNotAllowed(host.to_string()));
        }

        let api_key = get_api_key("anthropic")
            .ok_or_else(|| ExtractError::MissingApiKey("anthropic".to_string()))?;

        // If doc is already structured JSON, parse directly
        if let Ok(ext) = parse_invoice(doc) {
            return Ok(ext);
        }

        #[cfg(feature = "byok")]
        {
            let prompt = format!(
                "Extract structured invoice fields from file '{}'. Return valid JSON matching DocumentExtraction.",
                doc.name
            );

            let body = serde_json::json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{
                    "role": "user",
                    "content": prompt
                }]
            });

            let resp = ureq::post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .send_json(body)
                .map_err(|e| ExtractError::RequestFailed("anthropic".to_string(), e.to_string()))?;

            let resp_json: serde_json::Value = resp
                .into_body()
                .read_json()
                .map_err(|e| ExtractError::ParseError(e.to_string()))?;

            let text = resp_json["content"][0]["text"].as_str().unwrap_or("");
            let sub_doc = Document {
                mime: "application/json".to_string(),
                bytes: text.as_bytes().to_vec(),
                name: doc.name.clone(),
            };
            parse_invoice(&sub_doc)
        }

        #[cfg(not(feature = "byok"))]
        {
            parse_invoice(doc)
        }
    }
}

pub struct OpenAiCompatExtractor {
    pub base_url: String,
    pub extra_hosts: Vec<String>,
}

impl Extractor for OpenAiCompatExtractor {
    fn id(&self) -> &'static str {
        "openai_compat"
    }

    fn extract(&self, doc: &Document) -> Result<DocumentExtraction, ExtractError> {
        let host = self.base_url.replace("http://", "").replace("https://", "");
        let host_name = host.split('/').next().unwrap_or(&host);
        if !is_host_allowed(host_name, &self.extra_hosts) {
            return Err(ExtractError::HostNotAllowed(host_name.to_string()));
        }

        if let Ok(ext) = parse_invoice(doc) {
            return Ok(ext);
        }

        #[cfg(feature = "byok")]
        {
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let api_key = get_api_key("openai").unwrap_or_else(|| "none".to_string());
            let prompt = format!(
                "Extract structured invoice fields from file '{}'. Return JSON matching DocumentExtraction.",
                doc.name
            );

            let body = serde_json::json!({
                "model": "gpt-4o",
                "messages": [{
                    "role": "user",
                    "content": prompt
                }],
                "response_format": { "type": "json_object" }
            });

            let resp = ureq::post(&url)
                .header("Authorization", &format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .send_json(body)
                .map_err(|e| ExtractError::RequestFailed("openai_compat".to_string(), e.to_string()))?;

            let resp_json: serde_json::Value = resp
                .into_body()
                .read_json()
                .map_err(|e| ExtractError::ParseError(e.to_string()))?;

            let text = resp_json["choices"][0]["message"]["content"].as_str().unwrap_or("");
            let sub_doc = Document {
                mime: "application/json".to_string(),
                bytes: text.as_bytes().to_vec(),
                name: doc.name.clone(),
            };
            parse_invoice(&sub_doc)
        }

        #[cfg(not(feature = "byok"))]
        {
            parse_invoice(doc)
        }
    }
}

// Canonical invoice parser / structured normalizer (no static mock values)
pub fn parse_invoice(doc: &Document) -> Result<DocumentExtraction, ExtractError> {
    use ring::digest::{Context, SHA256};
    let mut context = Context::new(&SHA256);
    context.update(&doc.bytes);
    let digest = context.finish();
    let sha256 = digest.as_ref().iter().map(|b| format!("{:02x}", b)).collect();

    // 1. Direct JSON extraction deserialization
    if let Ok(mut ext) = serde_json::from_slice::<DocumentExtraction>(&doc.bytes) {
        ext.source = DocumentSource {
            file: doc.name.clone(),
            sha256,
        };
        return Ok(ext);
    }

    // 2. Embedded JSON block inside response or markdown
    if let Ok(text) = std::str::from_utf8(&doc.bytes) {
        if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                if start < end {
                    if let Ok(mut ext) = serde_json::from_str::<DocumentExtraction>(&text[start..=end]) {
                        ext.source = DocumentSource {
                            file: doc.name.clone(),
                            sha256,
                        };
                        return Ok(ext);
                    }
                }
            }
        }
    }

    Err(ExtractError::ParseError(format!(
        "Unable to parse document '{}' as structured invoice data. Configure AI BYOK key or provide JSON.",
        doc.name
    )))
}

// Compatibility alias
pub fn mock_or_parse_invoice(doc: &Document) -> Result<DocumentExtraction, ExtractError> {
    parse_invoice(doc)
}
