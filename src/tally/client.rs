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

#[derive(Clone)]
pub struct TallyClient {
    config: TallyConfig,
    breaker: Arc<CircuitBreaker>,
    queue: Arc<Semaphore>,
    active_company: Arc<RwLock<Option<String>>>,
}

impl TallyClient {
    #[allow(dead_code)]
    pub fn config(&self) -> &TallyConfig {
        &self.config
    }
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

    pub async fn execute_raw_xml(
        &self,
        xml_payload: &str,
        deadline: Duration,
    ) -> Result<String, TallyError> {
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
        let active = self.get_active_company().await;
        let breaker = self.circuit_breaker_state().await;
        match self.list_companies().await {
            Ok(companies) => {
                let act = active.or_else(|| companies.first().cloned());
                Ok(TallyStatus {
                    reachable: true,
                    target_url: self.target_url(),
                    loaded_companies: companies,
                    active_company: act,
                    circuit_breaker: format!("{:?}", breaker),
                    error: None,
                })
            }
            Err(e) => Ok(TallyStatus {
                reachable: false,
                target_url: self.target_url(),
                loaded_companies: Vec::new(),
                active_company: active,
                circuit_breaker: format!("{:?}", breaker),
                error: Some(e.to_string()),
            }),
        }
    }

    pub async fn list_companies(&self) -> Result<Vec<String>, TallyError> {
        let xml = build_list_companies_xml();
        let deadline = Duration::from_secs(self.config.call_deadline_secs);
        let resp = self.execute_raw_xml(&xml, deadline).await?;
        Ok(parse_company_names(&resp))
    }

    pub async fn list_ledgers(&self, company: Option<&str>) -> Result<Vec<String>, TallyError> {
        let active = self.get_active_company().await;
        let discovered: Option<String>;
        let comp = if let Some(c) = company {
            c
        } else if let Some(ref a) = active {
            a.as_str()
        } else if !self.config.default_company.is_empty() {
            self.config.default_company.as_str()
        } else if let Ok(comps) = self.list_companies().await {
            if let Some(first) = comps.first() {
                discovered = Some(first.clone());
                discovered.as_deref().unwrap_or("")
            } else {
                return Err(TallyError::TallyResponseError(
                    "No company specified, active, or loaded in Tally".to_string(),
                ));
            }
        } else {
            return Err(TallyError::TallyResponseError(
                "No company specified or active".to_string(),
            ));
        };

        let xml = build_list_ledgers_xml(comp);
        let deadline = Duration::from_secs(self.config.call_deadline_secs);
        let resp = self.execute_raw_xml(&xml, deadline).await?;
        Ok(parse_ledger_names(&resp))
    }

    pub async fn trial_balance(
        &self,
        company: Option<&str>,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<String, TallyError> {
        let active = self.get_active_company().await;
        let discovered: Option<String>;
        let comp = if let Some(c) = company {
            c
        } else if let Some(ref a) = active {
            a.as_str()
        } else if !self.config.default_company.is_empty() {
            self.config.default_company.as_str()
        } else if let Ok(comps) = self.list_companies().await {
            if let Some(first) = comps.first() {
                discovered = Some(first.clone());
                discovered.as_deref().unwrap_or("")
            } else {
                return Err(TallyError::TallyResponseError(
                    "No company specified, active, or loaded in Tally".to_string(),
                ));
            }
        } else {
            return Err(TallyError::TallyResponseError(
                "No company specified or active".to_string(),
            ));
        };

        let xml = build_trial_balance_xml(comp, from_date, to_date);
        let deadline = Duration::from_secs(self.config.report_deadline_secs);
        self.execute_raw_xml(&xml, deadline).await
    }

    pub async fn day_book(
        &self,
        company: Option<&str>,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<String, TallyError> {
        let active = self.get_active_company().await;
        let discovered: Option<String>;
        let comp = if let Some(c) = company {
            c
        } else if let Some(ref a) = active {
            a.as_str()
        } else if !self.config.default_company.is_empty() {
            self.config.default_company.as_str()
        } else if let Ok(comps) = self.list_companies().await {
            if let Some(first) = comps.first() {
                discovered = Some(first.clone());
                discovered.as_deref().unwrap_or("")
            } else {
                return Err(TallyError::TallyResponseError(
                    "No company specified, active, or loaded in Tally".to_string(),
                ));
            }
        } else {
            return Err(TallyError::TallyResponseError(
                "No company specified or active".to_string(),
            ));
        };

        let xml = build_day_book_xml(comp, from_date, to_date);
        let deadline = Duration::from_secs(self.config.report_deadline_secs);
        self.execute_raw_xml(&xml, deadline).await
    }

    pub async fn post_voucher_xml(
        &self,
        xml_payload: &str,
    ) -> Result<crate::tally::builder::ImportResponse, TallyError> {
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
        matches!(
            tokio::time::timeout(timeout, TcpStream::connect(host_port)).await,
            Ok(Ok(_))
        )
    }

    pub async fn verify_voucher_exists(
        &self,
        company: &str,
        voucher_number: &str,
    ) -> Result<bool, TallyError> {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn validate_tally_target_host(target: &str) -> Result<(), TallyError> {
    let clean = target
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_only = clean
        .split(':')
        .next()
        .unwrap_or(clean)
        .split('/')
        .next()
        .unwrap_or(clean);

    // Block cloud instance metadata and broadcast addresses (SSRF prevention)
    if host_only == "169.254.169.254"
        || host_only == "metadata.google.internal"
        || host_only.starts_with("169.254.")
        || host_only == "0.0.0.0"
        || host_only == "255.255.255.255"
    {
        return Err(TallyError::ConnectionError(
            target.to_string(),
            "Connection to cloud metadata or broadcast addresses is prohibited for security"
                .to_string(),
        ));
    }

    Ok(())
}

// Low-overhead TCP HTTP sender using tokio::net::TcpStream with Content-Length and chunked support
async fn reqwest_like_send(
    url: &str,
    body: Vec<u8>,
    timeout: Duration,
) -> Result<Vec<u8>, TallyError> {
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
        "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: text/xml; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nUser-Agent: TallyMCP/1.0\r\nAccept: */*\r\n\r\n",
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
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    let mut content_length: Option<usize> = None;
    let mut is_chunked = false;

    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        let remaining_time = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining_time.is_zero() {
            return Err(TallyError::Timeout(timeout));
        }

        let n = match tokio::time::timeout(remaining_time, stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break, // EOF reached
            Ok(Ok(bytes_read)) => bytes_read,
            Ok(Err(e)) => return Err(TallyError::ConnectionError(url.to_string(), e.to_string())),
            Err(_) => return Err(TallyError::Timeout(timeout)),
        };

        response_buf.extend_from_slice(&chunk[..n]);

        // Check if we found the header separator
        if header_end.is_none() {
            if let Some(pos) = response_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                let header_str = String::from_utf8_lossy(&response_buf[..pos]);
                for line in header_str.lines() {
                    let line_lower = line.to_lowercase();
                    if line_lower.starts_with("content-length:") {
                        if let Some(val_str) = line.split(':').nth(1) {
                            if let Ok(cl) = val_str.trim().parse::<usize>() {
                                content_length = Some(cl);
                            }
                        }
                    } else if line_lower.starts_with("transfer-encoding:")
                        && line_lower.contains("chunked")
                    {
                        is_chunked = true;
                    }
                }
            } else if let Some(pos) = response_buf.windows(2).position(|w| w == b"\n\n") {
                header_end = Some(pos + 2);
                let header_str = String::from_utf8_lossy(&response_buf[..pos]);
                for line in header_str.lines() {
                    let line_lower = line.to_lowercase();
                    if line_lower.starts_with("content-length:") {
                        if let Some(val_str) = line.split(':').nth(1) {
                            if let Ok(cl) = val_str.trim().parse::<usize>() {
                                content_length = Some(cl);
                            }
                        }
                    } else if line_lower.starts_with("transfer-encoding:")
                        && line_lower.contains("chunked")
                    {
                        is_chunked = true;
                    }
                }
            }
        }

        // If we know header_end and content_length, check if we have received all body bytes
        if let (Some(h_end), Some(cl)) = (header_end, content_length) {
            if response_buf.len() >= h_end + cl {
                break;
            }
        }

        // If chunked, check if we have reached the terminal chunk
        if let Some(h_end) = header_end {
            if is_chunked
                && (response_buf[h_end..].ends_with(b"\r\n0\r\n\r\n")
                    || response_buf[h_end..].ends_with(b"0\r\n\r\n"))
            {
                break;
            }
        }
    }

    if let Some(pos) = header_end {
        if is_chunked {
            Ok(dechunk_http_body(&response_buf[pos..]))
        } else if let Some(cl) = content_length {
            let end = (pos + cl).min(response_buf.len());
            Ok(response_buf[pos..end].to_vec())
        } else {
            Ok(response_buf[pos..].to_vec())
        }
    } else {
        Ok(response_buf)
    }
}

pub fn dechunk_http_body(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let slice = &bytes[cursor..];
        let line_end = match slice.windows(2).position(|w| w == b"\r\n") {
            Some(p) => p,
            None => break,
        };
        let size_str = match std::str::from_utf8(&slice[..line_end]) {
            Ok(s) => s.trim(),
            Err(_) => break,
        };
        let chunk_size = match usize::from_str_radix(size_str.split(';').next().unwrap_or(""), 16) {
            Ok(s) => s,
            Err(_) => break,
        };
        if chunk_size == 0 {
            break;
        }
        let data_start = cursor + line_end + 2;
        let data_end = (data_start + chunk_size).min(bytes.len());
        result.extend_from_slice(&bytes[data_start..data_end]);
        cursor = data_end;
        if cursor + 2 <= bytes.len() && &bytes[cursor..cursor + 2] == b"\r\n" {
            cursor += 2;
        }
    }
    if result.is_empty() && !bytes.is_empty() {
        bytes.to_vec()
    } else {
        result
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
                let local = e.local_name();
                let tag = local.as_ref();
                if tag.eq_ignore_ascii_case(b"companyname")
                    || tag.eq_ignore_ascii_case(b"name")
                    || tag.eq_ignore_ascii_case(b"svcurrentcompany")
                    || tag.eq_ignore_ascii_case(b"currentcompany")
                {
                    in_name_tag = true;
                }

                if tag.eq_ignore_ascii_case(b"company") || tag.eq_ignore_ascii_case(b"companyname")
                {
                    for attr in e.attributes().flatten() {
                        let k = attr.key.local_name();
                        if k.as_ref().eq_ignore_ascii_case(b"name")
                            || k.as_ref().eq_ignore_ascii_case(b"companyname")
                        {
                            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                                let s = val.trim();
                                if !s.is_empty() && !names.contains(&s.to_string()) {
                                    names.push(s.to_string());
                                }
                            }
                        }
                    }
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
            Event::CData(e) if in_name_tag => {
                let text = String::from_utf8_lossy(&e);
                let s = text.trim();
                if !s.is_empty() && !names.contains(&s.to_string()) {
                    names.push(s.to_string());
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
                let local = e.local_name();
                let tag = local.as_ref();
                if tag.eq_ignore_ascii_case(b"dspaccname")
                    || tag.eq_ignore_ascii_case(b"ledgername")
                    || tag.eq_ignore_ascii_case(b"name")
                {
                    in_ledger = true;
                }

                if tag.eq_ignore_ascii_case(b"ledger") {
                    for attr in e.attributes().flatten() {
                        let k = attr.key.local_name();
                        if k.as_ref().eq_ignore_ascii_case(b"name")
                            || k.as_ref().eq_ignore_ascii_case(b"ledgername")
                        {
                            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                                let s = val.trim();
                                if !s.is_empty() && !ledgers.contains(&s.to_string()) {
                                    ledgers.push(s.to_string());
                                }
                            }
                        }
                    }
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
            Event::CData(e) if in_ledger => {
                let text = String::from_utf8_lossy(&e);
                let s = text.trim();
                if !s.is_empty() && !ledgers.contains(&s.to_string()) {
                    ledgers.push(s.to_string());
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
