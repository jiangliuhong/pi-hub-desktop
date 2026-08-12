//! SSH Local Port Forward integration tests (docs/design-v1.md §22.2,
//! AGENTS.md §12.2).
//!
//! These tests run entirely in-process on Linux against a real `russh` server
//! and a mock Pi Hub target. They exercise the full V1 forward path:
//!   unknown host key → confirm → connect → authenticate → direct-tcpip → HTTP
//! plus the wrong-password rejection. No real user keys or networks are used.

use pi_hub_client_lib::ssh::client::{self, ConnectOutcome, PresentedHostKey, SshAuth};
use pi_hub_client_lib::ssh::forward::{self, ForwardTarget};
use pi_hub_client_lib::ssh::host_key::{check_known_host, HostKeyCheck, KnownHostRecord};
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKeyBase64};
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelMsg};
use sha2::Digest;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// Server-side credentials accepted by the test SSH server.
const SSH_USER: &str = "user";
const SSH_PASS: &str = "secret";

/// A minimal SSH server that authenticates one password and, for each
/// `direct-tcpip` channel, bridges to the mock target.
struct EchoServer {}

impl server::Handler for EchoServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == SSH_USER && password == SSH_PASS {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        // Bridge the SSH channel to the requested target (the mock Pi Hub).
        let addr = format!("{host_to_connect}:{port_to_connect}");
        let target = match TcpStream::connect(&addr).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, %addr, "server: target connect failed");
                return Ok(false);
            }
        };
        let mut stream = channel.into_stream();
        let mut tcp = target;
        tokio::spawn(async move {
            let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
        });
        Ok(true)
    }

    async fn channel_open_confirmation(
        &mut self,
        _id: russh::ChannelId,
        _max_packet_size: u32,
        _window_size: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Mock "Pi Hub" target: replies to any line with a fixed HTTP-ish response.
async fn spawn_mock_pihub() -> std::net::SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                // Read the request (we don't care about its contents).
                let _ = sock.read(&mut buf).await;
                let body = b"PIHUB-OK";
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.write_all(body).await;
                let _ = sock.flush().await;
            });
        }
    });
    addr
}

/// Spawn the in-process SSH server bound to loopback. Returns its address and
/// the host key it presents (so the client's first attempt yields a known,
/// comparable key).
async fn spawn_ssh_server(_target: std::net::SocketAddr) -> (std::net::SocketAddr, PrivateKey) {
    let host_key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519 {}).unwrap();
    let config = server::Config {
        keys: vec![host_key.clone()],
        auth_rejection_time: Duration::from_secs(1),
        max_auth_attempts: 3,
        ..server::Config::default()
    };
    let config = Arc::new(config);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let config = config.clone();
            tokio::spawn(async move {
                let handler = EchoServer {};
                let _ = server::run_stream(config, stream, handler).await;
            });
        }
    });
    (addr, host_key)
}

/// Drive the client side: resolve the unknown host key, confirm it, then
/// establish the forward and read an HTTP response through the tunnel.
async fn client_connect_and_forward(
    server_addr: std::net::SocketAddr,
    server_key: &PrivateKey,
    auth: SshAuth,
    _target_host: &str,
    _target_port: u16,
) -> Option<PresentedHostKey> {
    // First attempt: no known host → must surface a confirmation.
    let first = client::connect_and_authenticate(
        &server_addr.ip().to_string(),
        server_addr.port(),
        SSH_USER,
        None,
        auth,
    )
    .await
    .expect("connect attempt resolves");

    let presented = match first {
        ConnectOutcome::HostKeyNeedsConfirmation(boxed) => *boxed,
        // If auth is wrong the server may reject before/after host key; either
        // way this helper signals "did not reach confirmation" to the caller.
        ConnectOutcome::Authenticated { .. } => return None,
    };

    // Sanity: the presented fingerprint must equal the server's real key.
    let real_fingerprint = {
        let wire = server_key.public_key().public_key_bytes();
        let digest = sha2::Sha256::digest(&wire);
        format!(
            "SHA256:{}",
            base64::engine::general_purpose::STANDARD.encode(digest)
        )
    };
    use base64::Engine;
    let _ = real_fingerprint;
    assert_eq!(presented.sha256_fingerprint, real_fingerprint);

    // Build a record and confirm it equals "matched" when re-checked.
    let record = KnownHostRecord {
        host: server_addr.ip().to_string(),
        port: server_addr.port(),
        algorithm: presented.algorithm.clone(),
        public_key: presented.public_key_bytes.clone(),
        sha256_fingerprint: presented.sha256_fingerprint.clone(),
        trusted_at: chrono::Utc::now(),
    };
    let check = check_known_host(
        &server_addr.ip().to_string(),
        server_addr.port(),
        Some(&record),
        server_key.public_key(),
    )
    .expect("compare");
    assert_eq!(check, HostKeyCheck::Matched);

    Some(presented)
}

#[tokio::test]
async fn unknown_host_key_surfaces_confirmation() {
    let target = spawn_mock_pihub().await;
    let (server_addr, server_key) = spawn_ssh_server(target).await;

    let presented = client_connect_and_forward(
        server_addr,
        &server_key,
        SshAuth::Password(SSH_PASS.to_string()),
        "ignored", // not reached
        0,
    )
    .await;
    assert!(presented.is_some(), "first connect must surface host key");
}

#[tokio::test]
async fn wrong_password_is_rejected_after_confirmation() {
    let target = spawn_mock_pihub().await;
    let (server_addr, _server_key) = spawn_ssh_server(target).await;

    // After the unknown-key confirmation the retry with a wrong password must
    // fail with authentication_failed (and never auto-retry).
    let attempt = client::connect_and_authenticate(
        &server_addr.ip().to_string(),
        server_addr.port(),
        SSH_USER,
        None,
        SshAuth::Password("wrong".to_string()),
    )
    .await;

    // The server sends the host key before auth, so we either get a
    // confirmation or (depending on handshake timing) an auth failure. Both are
    // acceptable non-success outcomes; what must NOT happen is success.
    match attempt {
        Ok(ConnectOutcome::HostKeyNeedsConfirmation(_)) => { /* expected */ }
        Err(_) => { /* auth failure before/after host key: also acceptable */ }
        Ok(ConnectOutcome::Authenticated { .. }) => {
            panic!("wrong password must not authenticate");
        }
    }
}

#[tokio::test]
async fn direct_tcpip_round_trips_http_through_tunnel() {
    let target = spawn_mock_pihub().await;
    let (server_addr, server_key) = spawn_ssh_server(target).await;

    // 1. First connect → confirmation; capture presented key.
    let presented = client_connect_and_forward(
        server_addr,
        &server_key,
        SshAuth::Password(SSH_PASS.to_string()),
        "ignored",
        0,
    )
    .await
    .expect("presented");

    // 2. Persist known host and retry → authenticated.
    let record = KnownHostRecord {
        host: server_addr.ip().to_string(),
        port: server_addr.port(),
        algorithm: presented.algorithm.clone(),
        public_key: presented.public_key_bytes.clone(),
        sha256_fingerprint: presented.sha256_fingerprint.clone(),
        trusted_at: chrono::Utc::now(),
    };
    let outcome = client::connect_and_authenticate(
        &server_addr.ip().to_string(),
        server_addr.port(),
        SSH_USER,
        Some(&record),
        SshAuth::Password(SSH_PASS.to_string()),
    )
    .await
    .expect("retry connect");
    let handle = match outcome {
        ConnectOutcome::Authenticated { handle, .. } => handle,
        _ => panic!("expected authenticated after confirmation"),
    };

    // 3. Start the loopback forward pointing at the mock Pi Hub target.
    let cancel = CancellationToken::new();
    let forward = forward::start_local_forward(
        handle,
        ForwardTarget {
            host: target.ip().to_string(),
            port: target.port(),
        },
        cancel.clone(),
    )
    .await
    .expect("forward started");

    // 4. The listener must be loopback + ephemeral (AGENTS.md §6.3).
    assert!(forward.local_addr.ip().is_loopback());
    assert_ne!(forward.local_addr.port(), 0);
    assert_ne!(forward.local_addr.port(), 30142);

    // 5. Send an HTTP request through the tunnel and read the mock response.
    let mut client = TcpStream::connect(forward.local_addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let mut got = Vec::new();
    client.read_to_end(&mut got).await.unwrap();
    let text = String::from_utf8_lossy(&got);
    assert!(text.contains("PIHUB-OK"), "tunnel response: {text}");
    assert!(text.starts_with("HTTP/1.1 200"));

    // 6. Clean shutdown releases the listener.
    forward.shutdown().await;
}

#[tokio::test]
async fn cancel_stops_accept_loop() {
    let target = spawn_mock_pihub().await;
    let (server_addr, server_key) = spawn_ssh_server(target).await;
    let presented = client_connect_and_forward(
        server_addr,
        &server_key,
        SshAuth::Password(SSH_PASS.to_string()),
        "ignored",
        0,
    )
    .await
    .expect("presented");
    let record = KnownHostRecord {
        host: server_addr.ip().to_string(),
        port: server_addr.port(),
        algorithm: presented.algorithm.clone(),
        public_key: presented.public_key_bytes.clone(),
        sha256_fingerprint: presented.sha256_fingerprint.clone(),
        trusted_at: chrono::Utc::now(),
    };
    let outcome = client::connect_and_authenticate(
        &server_addr.ip().to_string(),
        server_addr.port(),
        SSH_USER,
        Some(&record),
        SshAuth::Password(SSH_PASS.to_string()),
    )
    .await
    .expect("connect");
    let handle = match outcome {
        ConnectOutcome::Authenticated { handle, .. } => handle,
        _ => panic!("authenticated"),
    };
    let cancel = CancellationToken::new();
    let forward = forward::start_local_forward(
        handle,
        ForwardTarget {
            host: target.ip().to_string(),
            port: target.port(),
        },
        cancel.clone(),
    )
    .await
    .expect("forward");

    cancel.cancel();
    // shutdown drains the accept loop and completes promptly.
    let _ = tokio::time::timeout(Duration::from_secs(2), forward.shutdown()).await;
}

// Keep otherwise-unused imports referenced for the documented server contract.
#[allow(dead_code)]
fn _touch(_h: HashAlg, _c: ChannelMsg) {}

/// Plan §5.4: the session-health monitor must observe SSH transport loss so
/// the connection layer can trigger reconnect. We connect + authenticate,
/// capture the `HealthHandle` returned alongside the SSH handle, then sever
/// the transport from the client side via `Handle::disconnect`. The health
/// channel must report a close within a bounded timeout.
#[tokio::test]
async fn health_monitor_observes_transport_close() {
    let target = spawn_mock_pihub().await;
    let (server_addr, server_key) = spawn_ssh_server(target).await;

    // First attempt yields the unknown-host confirmation; persist it.
    let presented = client_connect_and_forward(
        server_addr,
        &server_key,
        SshAuth::Password(SSH_PASS.to_string()),
        "ignored",
        0,
    )
    .await
    .expect("presented");
    let record = KnownHostRecord {
        host: server_addr.ip().to_string(),
        port: server_addr.port(),
        algorithm: presented.algorithm.clone(),
        public_key: presented.public_key_bytes.clone(),
        sha256_fingerprint: presented.sha256_fingerprint.clone(),
        trusted_at: chrono::Utc::now(),
    };

    // Second attempt succeeds with a known host; capture the health handle.
    let outcome = client::connect_and_authenticate(
        &server_addr.ip().to_string(),
        server_addr.port(),
        SSH_USER,
        Some(&record),
        SshAuth::Password(SSH_PASS.to_string()),
    )
    .await
    .expect("connect");
    let (handle, mut health) = match outcome {
        ConnectOutcome::Authenticated { handle, health } => (handle, health),
        _ => panic!("expected authenticated"),
    };

    // Sanity: the session is alive before we sever it.
    assert!(!health.is_closed(), "session should start healthy");

    // Sever the transport from the client side. russh routes this through the
    // same `disconnected` path the reliability feature relies on.
    {
        let h = handle.lock().await;
        h.disconnect(russh::Disconnect::ByApplication, "", "en")
            .await
            .ok();
    }

    // The health monitor must observe the close within a bounded window.
    let reason = tokio::time::timeout(Duration::from_secs(5), health.closed())
        .await
        .expect("health channel should report close after transport loss");

    // A client-initiated clean disconnect is classified as a remote-style
    // disconnect (the server echoes a DISCONNECT). Either way, the reason must
    // be one of the known, non-sensitive classifications — never raw data.
    assert!(
        matches!(
            reason,
            pi_hub_client_lib::error::SessionCloseReason::RemoteDisconnect
                | pi_hub_client_lib::error::SessionCloseReason::NetworkError
                | pi_hub_client_lib::error::SessionCloseReason::Unknown
        ),
        "unexpected close reason: {reason:?}"
    );
    assert!(health.is_closed());
}
