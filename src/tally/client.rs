use crate::config::TallyConfig;
use crate::tally::breaker::{BreakerState, CircuitBreaker};
use crate::tally::envelope::*;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore};

#[derive(Error, Debug)]
pub enum TallyError {
    #[error("Circuit breaker is OPEN. Tally is currently unresponsive.")]
    CircuitBreakerOpen,
    #[error("Tally gateway is busy (queue full). Please retry shortly.")]
    GatewayBusy,
    #[error("Failed to connect to Tally at {0}: {1}")]
    ConnectionError(String, String),
    #[error("Request to Tally timed out after {0:?}")]
    Timeout(Duration),
    #[allow(dead_code)]
    #[error("XML parsing error: {0}")]
    XmlParseError(String),
    #[error("Tally returned error: {0}")]
    TallyResponseError(String),
}

pub struct TallyClient {
    config: TallyConfig,
    breaker: Arc<CircuitBreaker>,
    queue: Arc<Semaphore>,
    active_company: Arc<RwLock<Option<String>>>,
}

impl TallyClient {
    pub fn new(config: TallyConfig) -> Self {
        let queue_depth = config.queue_depth;
        let breaker_failures = config.breaker_failures;
        Self {
            config,
            breaker: Arc::new(CircuitBreaker::new(breaker_failures, 30)),
            queue: Arc::new(Semaphore::new(queue_depth)),
            active_company: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_active_company(&self, company: String) {
        let mut lock = self.active_company.write().await;
        *lock = Some(company);
    }

    pub async fn get_active_company(&self) -> Option<String> {
        self.active_company.read().await.clone()
    }

    pub async fn circuit_breaker_state(&self) -> BreakerState {
        self.breaker.state().await
    }

    pub fn target_url(&self) -> String {
        format!("http://{}:{}", self.config.host, self.config.port)
    }

    pub async fn execute_raw_xml(&self, xml_payload: &str, deadline: Duration) -> Result<String, TallyError> {
        if !self.breaker.can_execute().await {
            return Err(TallyError::CircuitBreakerOpen);
        }

        // Bounded queue: try to acquire permit
        let _permit = self
            .queue
            .try_acquire()
            .map_err(|_| TallyError::GatewayBusy)?;

        let url = self.target_url();

        // Prepare request bytes (UTF-16LE with BOM if necessary, or UTF-8)
        let body_bytes = xml_payload.as_bytes().to_vec();

        let client = reqwest_like_send(&url, body_bytes, deadline).await;

        match client {
            Ok(resp_bytes) => {
                self.breaker.record_success().await;
                let decoded = decode_tally_response(&resp_bytes);
                Ok(decoded)
            }
            Err(err) => {
                self.breaker.record_failure().await;
                Err(err)
            }
        }
    }

    pub async fn status(&self) -> Result<TallyStatus, TallyError> {
        let companies = self.list_companies().await?;
        let active = self.get_active_company().await;
        let breaker = self.circuit_breaker_state().await;
        Ok(TallyStatus {
            reachable: true,
            target_url: self.target_url(),
            loaded_companies: companies,
            active_company: active,
            circuit_breaker: format!("{:?}", breaker),
        })
    }

    pub async fn list_companies(&self) -> Result<Vec<String>, TallyError> {
        let xml = build_list_companies_xml();
        let deadline = Duration::from_secs(self.config.call_deadline_secs);
        let resp = self.execute_raw_xml(&xml, deadline).await?;
        Ok(parse_company_names(&resp))
    }

    pub async fn list_ledgers(&self, company: Option<&str>) -> Result<Vec<String>, TallyError> {
        let active = self.get_active_company().await;
        let comp = company
            .or(active.as_deref())
            .or(if !self.config.default_company.is_empty() {
                Some(self.config.default_company.as_str())
            } else {
                None
            })
            .ok_or_else(|| TallyError::TallyResponseError("No company specified or active".to_string()))?;

        let xml = build_list_ledgers_xml(comp);
        let deadline = Duration::from_secs(self.config.call_deadline_secs);
        let resp = self.execute_raw_xml(&xml, deadline).await?;
        Ok(parse_ledger_names(&resp))
    }

    pub async fn trial_balance(&self, company: Option<&str>, from_date: Option<&str>, to_date: Option<&str>) -> Result<String, TallyError> {
        let active = self.get_active_company().await;
        let comp = company
            .or(active.as_deref())
            .ok_or_else(|| TallyError::TallyResponseError("No company specified or active".to_string()))?;

        let xml = build_trial_balance_xml(comp, from_date, to_date);
        let deadline = Duration::from_secs(self.config.report_deadline_secs);
        self.execute_raw_xml(&xml, deadline).await
    }

    pub async fn day_book(&self, company: Option<&str>, from_date: Option<&str>, to_date: Option<&str>) -> Result<String, TallyError> {
        let active = self.get_active_company().await;
        let comp = company
            .or(active.as_deref())
            .ok_or_else(|| TallyError::TallyResponseError("No company specified or active".to_string()))?;

        let xml = build_day_book_xml(comp, from_date, to_date);
        let deadline = Duration::from_secs(self.config.report_deadline_secs);
        self.execute_raw_xml(&xml, deadline).await
    }

    pub async fn post_voucher_xml(&self, xml_payload: &str) -> Result<crate::tally::builder::ImportResponse, TallyError> {
        let deadline = Duration::from_secs(self.config.call_deadline_secs);
        let response_xml = self.execute_raw_xml(xml_payload, deadline).await?;
        let parsed = crate::tally::builder::parse_import_response(&response_xml);
        Ok(parsed)
    }

    #[allow(dead_code)]
    pub async fn check_reachability(&self, timeout: Duration) -> bool {
        use tokio::net::TcpStream;
        let target = self.target_url().trim_start_matches("http://").to_string();
        let host_port = if let Some(idx) = target.find('/') {
            target[..idx].to_string()
        } else {
            target
        };
        matches!(tokio::time::timeout(timeout, TcpStream::connect(host_port)).await, Ok(Ok(_)))
    }

    pub async fn verify_voucher_exists(&self, company: &str, voucher_number: &str) -> Result<bool, TallyError> {
        // Query Day Book for the current company and check if voucher number is present
        let xml = build_day_book_xml(company, None, None);
        let deadline = Duration::from_secs(self.config.report_deadline_secs);
        let day_book_xml = self.execute_raw_xml(&xml, deadline).await?;
        Ok(day_book_xml.contains(voucher_number))
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TallyStatus {
    pub reachable: bool,
    pub target_url: String,
    pub loaded_companies: Vec<String>,
    pub active_company: Option<String>,
    pub circuit_breaker: String,
}

pub fn validate_tally_target_host(target: &str) -> Result<(), TallyError> {
    let clean = target.trim().trim_start_matches("http://").trim_start_matches("https://");
    let host_only = clean.split(':').next().unwrap_or(clean).split('/').next().unwrap_or(clean);

    // Block cloud instance metadata and broadcast addresses (SSRF prevention)
    if host_only == "169.254.169.254"
        || host_only == "metadata.google.internal"
        || host_only.starts_with("169.254.")
        || host_only == "0.0.0.0"
        || host_only == "255.255.255.255"
    {
        return Err(TallyError::ConnectionError(
            target.to_string(),
            "Connection to cloud metadata or broadcast addresses is prohibited for security".to_string(),
        ));
    }

    Ok(())
}

// Low-overhead TCP HTTP sender using tokio::net::TcpStream for zero heavyweight dependencies
async fn reqwest_like_send(url: &str, body: Vec<u8>, timeout: Duration) -> Result<Vec<u8>, TallyError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let target = url.trim_start_matches("http://");
    let host_port = if let Some(idx) = target.find('/') {
        &target[..idx]
    } else {
        target
    };

    validate_tally_target_host(host_port)?;

    let connect_future = TcpStream::connect(host_port);
    let mut stream = tokio::time::timeout(timeout, connect_future)
        .await
        .map_err(|_| TallyError::Timeout(timeout))?
        .map_err(|e| TallyError::ConnectionError(url.to_string(), e.to_string()))?;

    let header = format!(
        "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: text/xml; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        host_port,
        body.len()
    );

    let write_future = async {
        stream.write_all(header.as_bytes()).await?;
        stream.write_all(&body).await?;
        stream.flush().await?;
        Ok::<(), std::io::Error>(())
    };

    tokio::time::timeout(timeout, write_future)
        .await
        .map_err(|_| TallyError::Timeout(timeout))?
        .map_err(|e| TallyError::ConnectionError(url.to_string(), e.to_string()))?;

    let mut response_buf = Vec::new();
    let read_future = stream.read_to_end(&mut response_buf);

    tokio::time::timeout(timeout, read_future)
        .await
        .map_err(|_| TallyError::Timeout(timeout))?
        .map_err(|e| TallyError::ConnectionError(url.to_string(), e.to_string()))?;

    // Separate HTTP header from body
    if let Some(pos) = response_buf.windows(4).position(|w| w == b"\r\n\r\n") {
        Ok(response_buf[pos + 4..].to_vec())
    } else {
        Ok(response_buf)
    }
}

pub fn decode_tally_response(bytes: &[u8]) -> String {
    // Check for UTF-16LE BOM (0xFF, 0xFE)
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16_slice: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16_lossy(&u16_slice);
    }
    // Check for UTF-16BE BOM (0xFE, 0xFF)
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let u16_slice: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16_lossy(&u16_slice);
    }

    String::from_utf8_lossy(bytes).to_string()
}

pub fn parse_company_names(xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut in_name_tag = false;

    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(ref e) => {
                let name = e.name();
                if name.as_ref() == b"COMPANYNAME" || name.as_ref() == b"NAME" {
                    in_name_tag = true;
                }
            }
            Event::Text(e) if in_name_tag => {
                if let Ok(text) = e.unescape() {
                    let s = text.trim();
                    if !s.is_empty() && !names.contains(&s.to_string()) {
                        names.push(s.to_string());
                    }
                }
                in_name_tag = false;
            }
            Event::End(_) => {
                in_name_tag = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    names
}

pub fn parse_ledger_names(xml: &str) -> Vec<String> {
    let mut ledgers = Vec::new();
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut in_ledger = false;

    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(ref e) => {
                if e.name().as_ref() == b"DSPACCNAME" || e.name().as_ref() == b"LEDGERNAME" {
                    in_ledger = true;
                }
            }
            Event::Text(e) if in_ledger => {
                if let Ok(text) = e.unescape() {
                    let s = text.trim();
                    if !s.is_empty() && !ledgers.contains(&s.to_string()) {
                        ledgers.push(s.to_string());
                    }
                }
                in_ledger = false;
            }
            Event::End(_) => {
                in_ledger = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    ledgers
}
