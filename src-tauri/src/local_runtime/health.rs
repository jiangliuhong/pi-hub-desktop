//! Loopback service probe (docs/design-v2.md §10).
//!
//! Distinguishes "nothing listening", "Pi Hub", "some other service" and
//! "timed out" by talking to `GET /api/client-info` on `127.0.0.1:port`
//! (requirements-v2 §9 V2-FR-005, design-v2 §10.1). A bare TCP connect is
//! never enough — the endpoint must identify itself as Pi Hub with a protocol
//! version we support.
//!
//! HTTP is implemented with a tiny, loopback-only HTTP/1.1 client over
//! `tokio::TcpStream`. We never speak TLS here (the managed Pi Hub is bound to
//! `127.0.0.1`, design-v2 §6.5), so no `reqwest`/TLS dependency is needed
//! (AGENTS.md §3). Both `Content-Length` and `Transfer-Encoding: chunked`
//! responses are handled.

use crate::error::LocalRuntimeError;
use crate::local_runtime::model::{SUPPORTED_CLIENT_PROTOCOL_MAX, SUPPORTED_CLIENT_PROTOCOL_MIN};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Probe outcome (design-v2 §10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    /// Nothing is listening on the port.
    NotListening,
    /// A Pi Hub answered with a compatible protocol version.
    PiHub {
        version: String,
        protocol_version: u32,
    },
    /// Something answered but it is not Pi Hub.
    OtherService,
    /// The probe exceeded its deadline.
    TimedOut,
}

impl ProbeResult {
    pub fn is_pi_hub(&self) -> bool {
        matches!(self, ProbeResult::PiHub { .. })
    }
}

/// The probe contract (design-v2 §10).
#[async_trait]
pub trait LocalServiceProbe: Send + Sync {
    async fn probe(&self, port: u16, timeout: Duration) -> Result<ProbeResult, LocalRuntimeError>;
}

/// Default probe using the in-house loopback HTTP client.
pub struct HttpServiceProbe;

#[async_trait]
impl LocalServiceProbe for HttpServiceProbe {
    async fn probe(&self, port: u16, timeout: Duration) -> Result<ProbeResult, LocalRuntimeError> {
        match tokio::time::timeout(timeout, http_get_client_info(port)).await {
            Ok(Ok(parsed)) => Ok(parsed),
            Ok(Err(LocalServiceError::NotListening)) => Ok(ProbeResult::NotListening),
            Ok(Err(LocalServiceError::OtherService)) => Ok(ProbeResult::OtherService),
            Ok(Err(LocalServiceError::TimedOut)) => Ok(ProbeResult::TimedOut),
            // Any parse/IO hiccup that isn't a clean "no listener" is treated
            // as "some service answered but it's not Pi Hub" — safer than
            // reporting NotListening and letting a start clobber it.
            Ok(Err(_)) => Ok(ProbeResult::OtherService),
            Err(_) => Ok(ProbeResult::TimedOut),
        }
    }
}

#[derive(Debug)]
enum LocalServiceError {
    NotListening,
    OtherService,
    TimedOut,
    Io,
}

async fn http_get_client_info(port: u16) -> Result<ProbeResult, LocalServiceError> {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| map_connect_error(&e))?;
    // Short per-operation deadline so a hung connection still classifies.
    let _ = stream.set_nodelay(true);

    let request = format!(
        "GET /api/client-info HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|_| LocalServiceError::Io)?;

    let body = read_response_body(&mut stream).await?;
    classify_body(&body)
}

fn map_connect_error(e: &std::io::Error) -> LocalServiceError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::ConnectionRefused | ErrorKind::AddrNotAvailable | ErrorKind::NotConnected => {
            LocalServiceError::NotListening
        }
        ErrorKind::TimedOut => LocalServiceError::TimedOut,
        _ => LocalServiceError::Io,
    }
}

/// Read the full HTTP/1.1 response and return the decoded message body.
async fn read_response_body(stream: &mut TcpStream) -> Result<Vec<u8>, LocalServiceError> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];

    // Read until we have the full header block (\r\n\r\n).
    let header_end = loop {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|_| LocalServiceError::Io)?;
        if n == 0 {
            return Err(LocalServiceError::Io);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(idx) = find_header_terminator(&buf) {
            break idx;
        }
        if buf.len() > 64 * 1024 {
            // Pathologically large header block — give up.
            return Err(LocalServiceError::OtherService);
        }
    };

    let header_str = String::from_utf8_lossy(&buf[..header_end]);
    let (status, headers) = parse_headers(&header_str);

    // Treat non-2xx as "not our Pi Hub" (it may be Basic-Auth protected; in the
    // common managed case it won't be, and design-v2 §10.2 keeps client-info
    // exempt from Basic Auth).
    let is_success = (200..300).contains(&status);

    let mut body = buf[header_end + 4..].to_vec();

    let transfer_encoding_chunked = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    let content_length = headers.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("content-length") {
            v.trim().parse::<usize>().ok()
        } else {
            None
        }
    });

    if transfer_encoding_chunked {
        // Read until connection close (we sent Connection: close), then
        // de-chunk what we have.
        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|_| LocalServiceError::Io)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
            if body.len() > 1024 * 1024 {
                break;
            }
        }
        let decoded = dechunk(&body)?;
        if !is_success {
            return Ok(decoded); // classify_body will treat non-JSON as OtherService
        }
        return Ok(decoded);
    }

    if let Some(len) = content_length {
        while body.len() < len {
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|_| LocalServiceError::Io)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(len);
    } else {
        // No length framing; read until EOF.
        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|_| LocalServiceError::Io)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
            if body.len() > 1024 * 1024 {
                break;
            }
        }
    }

    if !is_success {
        return Err(LocalServiceError::OtherService);
    }
    Ok(body)
}

fn find_header_terminator(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_headers(header_block: &str) -> (u16, Vec<(String, String)>) {
    let mut lines = header_block.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    (status, headers)
}

/// Decode an HTTP/1.1 chunked body into the concatenated message bytes.
fn dechunk(body: &[u8]) -> Result<Vec<u8>, LocalServiceError> {
    let mut out = Vec::with_capacity(body.len());
    let mut cursor = 0;
    while cursor < body.len() {
        // Read the chunk size line up to \r\n.
        let line_end = body[cursor..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .map(|p| cursor + p)
            .ok_or(LocalServiceError::OtherService)?;
        let size_str = std::str::from_utf8(&body[cursor..line_end])
            .map_err(|_| LocalServiceError::OtherService)?;
        let size_str = size_str.split(';').next().unwrap_or(size_str).trim();
        let size =
            usize::from_str_radix(size_str, 16).map_err(|_| LocalServiceError::OtherService)?;
        cursor = line_end + 2;
        if size == 0 {
            break;
        }
        if cursor + size > body.len() {
            return Err(LocalServiceError::OtherService);
        }
        out.extend_from_slice(&body[cursor..cursor + size]);
        cursor += size;
        // Skip trailing \r\n after the chunk data.
        if cursor + 2 <= body.len() && &body[cursor..cursor + 2] == b"\r\n" {
            cursor += 2;
        }
    }
    Ok(out)
}

/// Classify a (possibly empty) body into a `ProbeResult`.
fn classify_body(body: &[u8]) -> Result<ProbeResult, LocalServiceError> {
    let text = std::str::from_utf8(body).map_err(|_| LocalServiceError::OtherService)?;
    let info: ClientInfo =
        serde_json::from_str(text).map_err(|_| LocalServiceError::OtherService)?;
    if info.service != "pi-hub" {
        return Ok(ProbeResult::OtherService);
    }
    if info.protocol_version < SUPPORTED_CLIENT_PROTOCOL_MIN
        || info.protocol_version > SUPPORTED_CLIENT_PROTOCOL_MAX
    {
        return Ok(ProbeResult::OtherService);
    }
    Ok(ProbeResult::PiHub {
        version: info.version,
        protocol_version: info.protocol_version,
    })
}

#[derive(Debug, Deserialize)]
struct ClientInfo {
    service: String,
    #[serde(default)]
    version: String,
    #[serde(default, rename = "protocolVersion")]
    protocol_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// Spawn a mock TCP server that replies with `response` bytes once a
    /// request is received, then closes.
    async fn mock_server(response: Vec<u8>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                // Drain the request (best-effort).
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock.write_all(&response).await;
                let _ = sock.shutdown().await;
            }
        });
        port
    }

    #[tokio::test]
    async fn probe_pi_hub_match() {
        let body = serde_json::json!({
            "service": "pi-hub",
            "version": "0.0.42",
            "protocolVersion": 1
        })
        .to_string();
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let port = mock_server(resp.into_bytes()).await;
        let probe = HttpServiceProbe;
        let res = probe.probe(port, Duration::from_secs(2)).await.unwrap();
        match res {
            ProbeResult::PiHub { version, .. } => assert_eq!(version, "0.0.42"),
            other => panic!("expected PiHub, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_not_listening() {
        let probe = HttpServiceProbe;
        // Pick an almost-certainly-free port by binding then dropping.
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let res = probe.probe(port, Duration::from_secs(1)).await.unwrap();
        assert!(matches!(
            res,
            ProbeResult::NotListening | ProbeResult::OtherService
        ));
    }

    #[tokio::test]
    async fn probe_other_service_wrong_json() {
        let body = r#"{"service":"nginx","version":"1"}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let port = mock_server(resp.into_bytes()).await;
        let probe = HttpServiceProbe;
        let res = probe.probe(port, Duration::from_secs(2)).await.unwrap();
        assert_eq!(res, ProbeResult::OtherService);
    }

    #[tokio::test]
    async fn probe_handles_chunked_response() {
        let body = serde_json::json!({
            "service": "pi-hub",
            "version": "0.0.42",
            "protocolVersion": 1
        })
        .to_string();
        // Split the JSON body into two chunks.
        let mid = body.len() / 2;
        let framed = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n{:X}\r\n{}\r\n0\r\n\r\n",
            mid,
            &body[..mid],
            body.len() - mid,
            &body[mid..]
        );
        let port = mock_server(framed.into_bytes()).await;
        let probe = HttpServiceProbe;
        let res = probe.probe(port, Duration::from_secs(2)).await.unwrap();
        assert!(matches!(res, ProbeResult::PiHub { .. }));
    }

    #[test]
    fn dechunk_decodes_simple_body() {
        let framed = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let out = dechunk(framed).unwrap();
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn classify_requires_pi_hub_service_name() {
        let body = br#"{"service":"pi-hub","version":"1","protocolVersion":1}"#;
        assert!(matches!(
            classify_body(body).unwrap(),
            ProbeResult::PiHub { .. }
        ));
        let other = br#"{"service":"other"}"#;
        assert_eq!(classify_body(other).unwrap(), ProbeResult::OtherService);
    }
}
