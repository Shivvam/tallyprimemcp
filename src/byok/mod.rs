#![allow(dead_code)]
use crate::models::{DocumentExtraction, DocumentSource};
use base64::Engine;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
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
    extra_hosts
        .iter()
        .any(|h| h.split(':').next().unwrap_or(h) == host_clean)
}

static MEMORY_KEYS: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

fn get_memory_store() -> &'static RwLock<HashMap<String, String>> {
    MEMORY_KEYS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn get_api_key(provider: &str) -> Option<String> {
    let p_clean = provider.trim().to_lowercase();
    if p_clean == "ollama" {
        return Some("ollama".to_string());
    }

    // 1. Try environment variable
    let env_var = match p_clean.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "AI_API_KEY",
    };

    if let Ok(val) = std::env::var(env_var) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    // 2. Try in-memory store
    if let Ok(store) = get_memory_store().read() {
        if let Some(val) = store.get(&p_clean) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    // 3. Try OS Credential Manager via keyring
    #[cfg(feature = "byok")]
    {
        if let Ok(entry) = keyring::Entry::new("TallyMCP", &p_clean) {
            if let Ok(secret) = entry.get_password() {
                let trimmed = secret.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

pub fn get_masked_api_key(provider: &str) -> Option<String> {
    let p_clean = provider.trim().to_lowercase();
    if p_clean == "ollama" {
        return Some("Local (No key required)".to_string());
    }
    get_api_key(&p_clean).map(|key| {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if trimmed.len() <= 8 {
            "••••••••".to_string()
        } else {
            let start = &trimmed[..4.min(trimmed.len())];
            let end = &trimmed[trimmed.len().saturating_sub(4)..];
            format!("{}••••{}", start, end)
        }
    })
}

pub fn set_api_key(provider: &str, key: &str) -> Result<(), String> {
    let p_clean = provider.trim().to_lowercase();
    let key_trimmed = key.trim().to_string();

    let env_var = match p_clean.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "AI_API_KEY",
    };
    std::env::set_var(env_var, &key_trimmed);

    if let Ok(mut store) = get_memory_store().write() {
        store.insert(p_clean.clone(), key_trimmed.clone());
    }

    #[cfg(feature = "byok")]
    {
        if let Ok(entry) = keyring::Entry::new("TallyMCP", &p_clean) {
            let _ = entry.set_password(&key_trimmed);
        }
    }

    Ok(())
}

pub struct AnthropicExtractor {
    pub extra_hosts: Vec<String>,
    pub model: String,
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
            let model_name = if self.model.trim().is_empty() {
                "claude-3-5-sonnet-20241022"
            } else {
                self.model.trim()
            };

            let prompt_text = "Extract structured invoice details. Return ONLY raw JSON matching DocumentExtraction with fields: doc_type, party_name, party_gstin, invoice_no, date (YYYY-MM-DD), currency, lines ([description, hsn, qty, rate, amount, tax_rate]), taxes ([type, rate, amount]), round_off, total, confidence ({total: 0.95}).";

            let user_content = if doc.mime.starts_with("image/") || doc.mime == "application/pdf" {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&doc.bytes);
                let media_type = if doc.mime == "application/pdf" {
                    "application/pdf"
                } else {
                    &doc.mime
                };
                let type_field = if doc.mime == "application/pdf" {
                    "document"
                } else {
                    "image"
                };
                serde_json::json!([
                    {
                        "type": type_field,
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": b64
                        }
                    },
                    {
                        "type": "text",
                        "text": prompt_text
                    }
                ])
            } else {
                let content_str = String::from_utf8_lossy(&doc.bytes);
                serde_json::json!([
                    {
                        "type": "text",
                        "text": format!("{}\n\nDocument contents:\n{}", prompt_text, content_str)
                    }
                ])
            };

            let body = serde_json::json!({
                "model": model_name,
                "max_tokens": 1500,
                "messages": [{
                    "role": "user",
                    "content": user_content
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
    pub model: String,
    pub api_key_provider: String,
    pub extra_hosts: Vec<String>,
}

impl Extractor for OpenAiCompatExtractor {
    fn id(&self) -> &'static str {
        "openai_compat"
    }

    fn extract(&self, doc: &Document) -> Result<DocumentExtraction, ExtractError> {
        let clean_url = self
            .base_url
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let host_name = clean_url.split('/').next().unwrap_or(clean_url);
        if !is_host_allowed(host_name, &self.extra_hosts) {
            return Err(ExtractError::HostNotAllowed(host_name.to_string()));
        }

        if let Ok(ext) = parse_invoice(doc) {
            return Ok(ext);
        }

        #[cfg(feature = "byok")]
        {
            let endpoint = if self.base_url.ends_with("/chat/completions") {
                self.base_url.clone()
            } else {
                format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
            };

            let is_ollama = self.api_key_provider == "ollama";
            let api_key = if is_ollama {
                "ollama".to_string()
            } else {
                get_api_key(&self.api_key_provider)
                    .ok_or_else(|| ExtractError::MissingApiKey(self.api_key_provider.clone()))?
            };

            if !is_ollama && (api_key.trim().is_empty() || api_key == "none") {
                return Err(ExtractError::MissingApiKey(self.api_key_provider.clone()));
            }

            let model_name = if self.model.trim().is_empty() {
                if is_ollama {
                    "llama3.2-vision"
                } else if self.api_key_provider == "gemini" {
                    "gemini-3.8-flash"
                } else {
                    "gpt-4o"
                }
            } else {
                self.model.trim()
            };

            let prompt_text = "Extract structured invoice details. Return ONLY raw JSON matching DocumentExtraction with fields: doc_type, party_name, party_gstin, invoice_no, date (YYYY-MM-DD), currency, lines ([description, hsn, qty, rate, amount, tax_rate]), taxes ([type, rate, amount]), round_off, total, confidence ({total: 0.95}).";

            let user_content = if doc.mime.starts_with("image/") {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&doc.bytes);
                serde_json::json!([
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", doc.mime, b64)
                        }
                    },
                    {
                        "type": "text",
                        "text": prompt_text
                    }
                ])
            } else {
                let content_str = String::from_utf8_lossy(&doc.bytes);
                serde_json::json!([
                    {
                        "type": "text",
                        "text": format!("{}\n\nDocument contents:\n{}", prompt_text, content_str)
                    }
                ])
            };

            let body = serde_json::json!({
                "model": model_name,
                "messages": [{
                    "role": "user",
                    "content": user_content
                }],
                "response_format": { "type": "json_object" }
            });

            let mut last_error = String::new();
            let mut resp_opt = None;

            // Retry up to 3 attempts with backoff for transient 503 / 429 from provider
            for attempt in 0..3 {
                let mut req = ureq::post(&endpoint).header("content-type", "application/json");

                if !is_ollama && api_key != "none" && !api_key.is_empty() {
                    req = req.header("Authorization", &format!("Bearer {}", api_key));
                }

                match req.send_json(body.clone()) {
                    Ok(resp) => {
                        resp_opt = Some(resp);
                        break;
                    }
                    Err(e) => {
                        last_error = e.to_string();
                        if (last_error.contains("503") || last_error.contains("429")) && attempt < 2
                        {
                            std::thread::sleep(std::time::Duration::from_millis(
                                1000 * (attempt + 1),
                            ));
                            continue;
                        }
                        break;
                    }
                }
            }

            let resp = match resp_opt {
                Some(r) => r,
                None => {
                    if last_error.contains("503") {
                        return Err(ExtractError::RequestFailed(
                            self.api_key_provider.clone(),
                            format!(
                                "Google Gemini service is temporarily overloaded / experiencing high demand (HTTP 503) for model '{}'. Spikes in demand are usually temporary. Please retry in a few moments, or check your API key / model in AI settings.",
                                model_name
                            ),
                        ));
                    }
                    return Err(ExtractError::RequestFailed(
                        self.api_key_provider.clone(),
                        last_error,
                    ));
                }
            };

            let resp_json: serde_json::Value = resp
                .into_body()
                .read_json()
                .map_err(|e| ExtractError::ParseError(e.to_string()))?;

            let text = resp_json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("");
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

pub fn extract_document(
    config: &crate::config::Config,
    doc: &Document,
) -> Result<DocumentExtraction, ExtractError> {
    if let Ok(ext) = parse_invoice(doc) {
        return Ok(ext);
    }

    let provider = config.ai.provider.trim().to_lowercase();
    let provider_str = if provider.is_empty() {
        "anthropic"
    } else {
        provider.as_str()
    };

    match provider_str {
        "anthropic" => {
            let extractor = AnthropicExtractor {
                extra_hosts: config.ai.extra_allowed_hosts.clone(),
                model: config.ai.model.clone(),
            };
            extractor.extract(doc)
        }
        "openai" => {
            let base_url = if config.ai.base_url.trim().is_empty() {
                "https://api.openai.com/v1".to_string()
            } else {
                config.ai.base_url.trim().to_string()
            };
            let extractor = OpenAiCompatExtractor {
                base_url,
                model: if config.ai.model.trim().is_empty() {
                    "gpt-4o".to_string()
                } else {
                    config.ai.model.trim().to_string()
                },
                api_key_provider: "openai".to_string(),
                extra_hosts: config.ai.extra_allowed_hosts.clone(),
            };
            extractor.extract(doc)
        }
        "ollama" => {
            let base_url = if config.ai.base_url.trim().is_empty() {
                "http://127.0.0.1:11434/v1".to_string()
            } else {
                config.ai.base_url.trim().to_string()
            };
            let extractor = OpenAiCompatExtractor {
                base_url,
                model: if config.ai.model.trim().is_empty() {
                    "llama3.2-vision".to_string()
                } else {
                    config.ai.model.trim().to_string()
                },
                api_key_provider: "ollama".to_string(),
                extra_hosts: config.ai.extra_allowed_hosts.clone(),
            };
            extractor.extract(doc)
        }
        "gemini" => {
            let base_url = if config.ai.base_url.trim().is_empty() {
                "https://generativelanguage.googleapis.com/v1beta/openai/".to_string()
            } else {
                config.ai.base_url.trim().to_string()
            };
            let extractor = OpenAiCompatExtractor {
                base_url,
                model: if config.ai.model.trim().is_empty() {
                    "gemini-3.8-flash".to_string()
                } else {
                    config.ai.model.trim().to_string()
                },
                api_key_provider: "gemini".to_string(),
                extra_hosts: config.ai.extra_allowed_hosts.clone(),
            };
            extractor.extract(doc)
        }
        _ => {
            let extractor = OpenAiCompatExtractor {
                base_url: config.ai.base_url.clone(),
                model: config.ai.model.clone(),
                api_key_provider: provider_str.to_string(),
                extra_hosts: config.ai.extra_allowed_hosts.clone(),
            };
            extractor.extract(doc)
        }
    }
}

// Canonical invoice parser / structured normalizer (no static mock values)
pub fn parse_invoice(doc: &Document) -> Result<DocumentExtraction, ExtractError> {
    use ring::digest::{Context, SHA256};
    let mut context = Context::new(&SHA256);
    context.update(&doc.bytes);
    let digest = context.finish();
    let sha256 = digest
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

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
                    if let Ok(mut ext) =
                        serde_json::from_str::<DocumentExtraction>(&text[start..=end])
                    {
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

/// Conversational chat with Tally context via configured AI provider (Anthropic, Gemini, OpenAI, Ollama)
pub fn chat(
    config: &crate::config::Config,
    prompt: &str,
    tally_context: &str,
) -> Result<String, ExtractError> {
    let provider = config.ai.provider.trim().to_lowercase();
    let provider_str = if provider.is_empty() {
        "anthropic"
    } else {
        provider.as_str()
    };

    let system_prompt = format!(
        "You are TallyMCP AI Assistant, an expert accountant and financial analyst. \
         You help users analyze, query, and understand their TallyPrime accounting data.\n\
         TallyPrime Live Context:\n{}\n\
         Instructions:\n\
         - Answer clearly, accurately, and concisely.\n\
         - Format financial figures, balances, and reports using Markdown tables or bulleted lists.\n\
         - If relevant Tally data is missing or Tally is offline, guide the user on which report or Tally menu to check.",
        tally_context
    );

    #[cfg(feature = "byok")]
    {
        if provider_str == "anthropic" {
            let host = "api.anthropic.com";
            if !is_host_allowed(host, &config.ai.extra_allowed_hosts) {
                return Err(ExtractError::HostNotAllowed(host.to_string()));
            }

            let api_key = get_api_key("anthropic")
                .ok_or_else(|| ExtractError::MissingApiKey("anthropic".to_string()))?;

            let model_name = if config.ai.model.trim().is_empty() {
                "claude-3-5-sonnet-20241022"
            } else {
                config.ai.model.trim()
            };

            let body = serde_json::json!({
                "model": model_name,
                "max_tokens": 2048,
                "system": system_prompt,
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

            if let Some(err_obj) = resp_json.get("error") {
                let msg = if let Some(m) = err_obj.get("message").and_then(|v| v.as_str()) {
                    m.to_string()
                } else if let Some(s) = err_obj.as_str() {
                    s.to_string()
                } else {
                    err_obj.to_string()
                };
                return Err(ExtractError::RequestFailed("anthropic".to_string(), msg));
            }

            let text_opt = resp_json["content"][0]["text"].as_str();
            let text = match text_opt {
                Some(t) => t.to_string(),
                None => {
                    if let Some(arr) = resp_json["content"].as_array() {
                        let mut combined = String::new();
                        for part in arr {
                            if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                                combined.push_str(txt);
                            }
                        }
                        if !combined.is_empty() {
                            combined
                        } else {
                            "No response text received from Claude.".to_string()
                        }
                    } else {
                        "No response text received from Claude.".to_string()
                    }
                }
            };
            return Ok(text);
        }

        // OpenAI / Gemini / Ollama compatible
        let (endpoint, model_name, api_key_provider, is_ollama) = match provider_str {
            "gemini" => {
                let base = if config.ai.base_url.trim().is_empty() {
                    "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                        .to_string()
                } else if config.ai.base_url.ends_with("/chat/completions") {
                    config.ai.base_url.trim().to_string()
                } else {
                    format!(
                        "{}/chat/completions",
                        config.ai.base_url.trim().trim_end_matches('/')
                    )
                };
                let m = if config.ai.model.trim().is_empty() {
                    "gemini-3.8-flash".to_string()
                } else {
                    config.ai.model.trim().to_string()
                };
                (base, m, "gemini".to_string(), false)
            }
            "openai" => {
                let base = if config.ai.base_url.trim().is_empty() {
                    "https://api.openai.com/v1/chat/completions".to_string()
                } else if config.ai.base_url.ends_with("/chat/completions") {
                    config.ai.base_url.trim().to_string()
                } else {
                    format!(
                        "{}/chat/completions",
                        config.ai.base_url.trim().trim_end_matches('/')
                    )
                };
                let m = if config.ai.model.trim().is_empty() {
                    "gpt-4o".to_string()
                } else {
                    config.ai.model.trim().to_string()
                };
                (base, m, "openai".to_string(), false)
            }
            "ollama" => {
                let base = if config.ai.base_url.trim().is_empty() {
                    "http://127.0.0.1:11434/v1/chat/completions".to_string()
                } else if config.ai.base_url.ends_with("/chat/completions") {
                    config.ai.base_url.trim().to_string()
                } else {
                    format!(
                        "{}/chat/completions",
                        config.ai.base_url.trim().trim_end_matches('/')
                    )
                };
                let m = if config.ai.model.trim().is_empty() {
                    "llama3.2".to_string()
                } else {
                    config.ai.model.trim().to_string()
                };
                (base, m, "ollama".to_string(), true)
            }
            _ => {
                let base = if config.ai.base_url.ends_with("/chat/completions") {
                    config.ai.base_url.trim().to_string()
                } else {
                    format!(
                        "{}/chat/completions",
                        config.ai.base_url.trim().trim_end_matches('/')
                    )
                };
                let m = if config.ai.model.trim().is_empty() {
                    "gpt-4o".to_string()
                } else {
                    config.ai.model.trim().to_string()
                };
                (base, m, provider_str.to_string(), false)
            }
        };

        let clean_url = endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let host_name = clean_url.split('/').next().unwrap_or(clean_url);
        if !is_host_allowed(host_name, &config.ai.extra_allowed_hosts) {
            return Err(ExtractError::HostNotAllowed(host_name.to_string()));
        }

        let api_key = if is_ollama {
            "ollama".to_string()
        } else {
            get_api_key(&api_key_provider)
                .ok_or_else(|| ExtractError::MissingApiKey(api_key_provider.clone()))?
        };

        let body = serde_json::json!({
            "model": model_name,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": prompt }
            ]
        });

        let mut last_error = String::new();
        let mut resp_opt = None;

        for attempt in 0..3 {
            let mut req = ureq::post(&endpoint).header("content-type", "application/json");
            if !is_ollama && !api_key.is_empty() && api_key != "none" {
                req = req.header("Authorization", &format!("Bearer {}", api_key));
            }

            match req.send_json(body.clone()) {
                Ok(resp) => {
                    resp_opt = Some(resp);
                    break;
                }
                Err(e) => {
                    last_error = e.to_string();
                    if (last_error.contains("503") || last_error.contains("429")) && attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(
                            1000 * (attempt as u64 + 1),
                        ));
                        continue;
                    }
                    break;
                }
            }
        }

        let resp = match resp_opt {
            Some(r) => r,
            None => {
                if last_error.contains("503") {
                    return Err(ExtractError::RequestFailed(
                        api_key_provider.clone(),
                        format!(
                            "Provider service temporarily overloaded (HTTP 503). Retried 3 times with backoff. Details: {}",
                            last_error
                        ),
                    ));
                }
                return Err(ExtractError::RequestFailed(
                    api_key_provider.clone(),
                    last_error,
                ));
            }
        };

        let resp_json: serde_json::Value = resp
            .into_body()
            .read_json()
            .map_err(|e| ExtractError::ParseError(e.to_string()))?;

        if let Some(err_obj) = resp_json.get("error") {
            let msg = if let Some(m) = err_obj.get("message").and_then(|v| v.as_str()) {
                m.to_string()
            } else if let Some(s) = err_obj.as_str() {
                s.to_string()
            } else {
                err_obj.to_string()
            };
            return Err(ExtractError::RequestFailed(api_key_provider.clone(), msg));
        }

        let text_opt = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| resp_json["choices"][0]["text"].as_str())
            .or_else(|| resp_json["candidates"][0]["content"]["parts"][0]["text"].as_str());

        let text = match text_opt {
            Some(t) if !t.trim().is_empty() => t.to_string(),
            _ => {
                if let Some(arr) = resp_json["choices"][0]["message"]["content"].as_array() {
                    let mut combined = String::new();
                    for part in arr {
                        if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                            combined.push_str(txt);
                        }
                    }
                    if !combined.is_empty() {
                        combined
                    } else if let Some(rc) =
                        resp_json["choices"][0]["message"]["reasoning_content"].as_str()
                    {
                        if !rc.trim().is_empty() {
                            rc.to_string()
                        } else {
                            "No response text received from model.".to_string()
                        }
                    } else {
                        "No response text received from model.".to_string()
                    }
                } else if let Some(parts) =
                    resp_json["candidates"][0]["content"]["parts"].as_array()
                {
                    let mut combined = String::new();
                    for part in parts {
                        if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                            combined.push_str(txt);
                        }
                    }
                    if !combined.is_empty() {
                        combined
                    } else {
                        "No response text received from model.".to_string()
                    }
                } else if let Some(rc) =
                    resp_json["choices"][0]["message"]["reasoning_content"].as_str()
                {
                    if !rc.trim().is_empty() {
                        rc.to_string()
                    } else {
                        "No response text received from model.".to_string()
                    }
                } else if let Some(refusal) = resp_json["choices"][0]["message"]["refusal"].as_str()
                {
                    format!("Model refusal: {}", refusal)
                } else if let Some(finish_reason) =
                    resp_json["choices"][0]["finish_reason"].as_str()
                {
                    if finish_reason != "stop" {
                        format!(
                            "No response content received from model (finish_reason: '{}').",
                            finish_reason
                        )
                    } else {
                        "No response text received from model.".to_string()
                    }
                } else {
                    "No response text received from model.".to_string()
                }
            }
        };
        Ok(text)
    }

    #[cfg(not(feature = "byok"))]
    {
        Ok(format!(
            "BYOK feature is disabled. Received prompt: {}",
            prompt
        ))
    }
}
