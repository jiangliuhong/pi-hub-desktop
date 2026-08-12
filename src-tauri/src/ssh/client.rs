//! SSH client: connect with strict host-key verification and authenticate
//! (docs/design-v1.md §7.3, §9; AGENTS.md §6.2, §7).
//!
//! Host-key policy is absolute (AGENTS.md §6.2):
//! - A presented key is compared against the known host for `(host, port)`.
//! - Match → proceed to authenticate.
//! - Unknown → the connection is aborted and the key facts are returned to the
//!   caller so it can drive the explicit first-time confirmation flow (FR-007).
//!   We never auto-accept.
//! - Changed → connection is blocked with `host_key_changed` (FR-008). The
//!   replacement flow is a separate, doubly-confirmed path.

use crate::error::{AppError, SshError};
use crate::ssh::health::{classify_disconnect, HealthHandle, HealthMonitor, HealthSignal};
use crate::ssh::host_key::{check_known_host, HostKeyCheck, HostKeyFacts, KnownHostRecord};
use russh::client::AuthResult;
use russh::client::{self, DisconnectReason, Handle};
use russh::keys::ssh_key::HashAlg;
use russh::keys::ssh_key::PublicKey as SshPublicKey;
use russh::keys::PrivateKey;
use russh::keys::PrivateKeyWithHashAlg;
use russh::keys::PublicKeyBase64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Default connect timeout. Distinct from keepalive (design §8.5).
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Credential used to authenticate an SSH session. Private keys must already
/// be decrypted (passphrase applied) by the caller via [`crate::ssh::key_loader`].
/// `PrivateKey` is boxed to keep the enum variants similarly sized.
pub enum SshAuth {
    Password(String),
    PrivateKey(Box<PrivateKey>),
}

/// Non-secret facts about a server-presented host key, surfaced for the
/// first-time confirmation UI (FR-007).
#[derive(Debug, Clone)]
pub struct PresentedHostKey {
    pub algorithm: String,
    pub sha256_fingerprint: String,
    pub public_key_bytes: Vec<u8>,
}

impl PresentedHostKey {
    fn from_key(key: &SshPublicKey) -> Self {
        let facts = HostKeyFacts::from_key(key);
        PresentedHostKey {
            algorithm: facts.algorithm,
            sha256_fingerprint: facts.sha256_fingerprint,
            public_key_bytes: key.public_key_bytes(),
        }
    }
}

/// Outcome of a connect attempt.
pub enum ConnectOutcome {
    /// Authenticated session ready for `direct-tcpip` forwarding. The
    /// [`HealthHandle`] observes transport loss so the caller can drive
    /// reconnect (plan §5.4).
    Authenticated {
        handle: Arc<Mutex<Handle<HostKeyVerifyingHandler>>>,
        health: HealthHandle,
    },
    /// Server key is not yet known. The caller must confirm `presented` out of
    /// band, persist a `KnownHostRecord`, then retry. Boxed to keep the enum
    /// variants similarly sized (clippy: large_enum_variant).
    HostKeyNeedsConfirmation(Box<PresentedHostKey>),
}

/// Internal record of the host-key decision made during the handshake.
/// `Changed` carries both old and new facts so the replacement flow can show
/// them side-by-side (FR-008); those fields are surfaced when the manager
/// drives the explicit replace path.
#[derive(Debug, Default)]
enum HostKeyDecision {
    #[default]
    Pending,
    Accepted,
    NeedsConfirm(PresentedHostKey),
    Changed {
        #[allow(dead_code)]
        expected: HostKeyFacts,
        #[allow(dead_code)]
        presented: PresentedHostKey,
    },
}

/// Client handler that only accepts a server key after explicit known-host
/// comparison. This is the single place a host key is trusted (AGENTS.md §6.2).
#[derive(Clone)]
pub struct HostKeyVerifyingHandler {
    host: String,
    port: u16,
    known: Option<KnownHostRecord>,
    decision: Arc<Mutex<HostKeyDecision>>,
    /// Session-health signal written from `disconnected()` (plan §5.4).
    health: HealthSignal,
}

impl HostKeyVerifyingHandler {
    fn new(
        host: &str,
        port: u16,
        known: Option<KnownHostRecord>,
        health: HealthSignal,
    ) -> (Self, Arc<Mutex<HostKeyDecision>>) {
        let decision = Arc::new(Mutex::new(HostKeyDecision::Pending));
        (
            HostKeyVerifyingHandler {
                host: host.to_string(),
                port,
                known,
                decision: decision.clone(),
                health,
            },
            decision,
        )
    }
}

impl client::Handler for HostKeyVerifyingHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &SshPublicKey,
    ) -> Result<bool, Self::Error> {
        let presented = PresentedHostKey::from_key(server_public_key);
        let check = check_known_host(
            &self.host,
            self.port,
            self.known.as_ref(),
            server_public_key,
        );
        let (accept, new_decision) = match check {
            Ok(HostKeyCheck::Matched) => (true, HostKeyDecision::Accepted),
            Ok(HostKeyCheck::Unknown(_)) => {
                (false, HostKeyDecision::NeedsConfirm(presented.clone()))
            }
            Ok(HostKeyCheck::Changed { expected, .. }) => (
                false,
                HostKeyDecision::Changed {
                    expected,
                    presented: presented.clone(),
                },
            ),
            Err(e) => {
                tracing::error!(error = ?e, "host key check failed");
                return Ok(false);
            }
        };
        *self.decision.lock().await = new_decision;
        Ok(accept)
    }

    async fn channel_open_confirmation(
        &mut self,
        _id: russh::ChannelId,
        _max_packet_size: u32,
        _window_size: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Observe session end (plan §5.4). russh calls this for both clean
    /// disconnects (`ReceivedDisconnect`) and error-driven closes — including
    /// `Error::KeepaliveTimeout` when keepalive probes go unanswered past the
    /// configured threshold. We classify the reason into a non-sensitive
    /// [`crate::error::SessionCloseReason`] and push it to the health channel
    /// so the connection supervisor can trigger reconnect. Returning `Ok`
    /// everywhere: the raw error is losslessly captured in the classified
    /// reason; re-emitting it would only surface as a generic
    /// `Error::Disconnect` at the russh call site.
    async fn disconnected(
        &mut self,
        reason: DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        let classified = classify_disconnect(&reason);
        tracing::info!(close_reason = classified.as_str(), "ssh session ended");
        self.health.report_close(classified);
        Ok(())
    }
}

/// Connect to `host:port`, enforce host-key policy, then authenticate.
///
/// `known` is the previously confirmed record for this endpoint, if any.
/// On a brand-new endpoint this returns `HostKeyNeedsConfirmation` after
/// capturing the presented key; the caller confirms, persists the record and
/// retries (FR-007).
pub async fn connect_and_authenticate(
    host: &str,
    port: u16,
    username: &str,
    known: Option<&KnownHostRecord>,
    auth: SshAuth,
) -> Result<ConnectOutcome, AppError> {
    connect_with_timeout(host, port, username, known, auth, DEFAULT_CONNECT_TIMEOUT).await
}

async fn connect_with_timeout(
    host: &str,
    port: u16,
    username: &str,
    known: Option<&KnownHostRecord>,
    auth: SshAuth,
    connect_timeout: Duration,
) -> Result<ConnectOutcome, AppError> {
    // 1. Resolve DNS explicitly so a resolution failure maps to `dns_failed`
    //    rather than a generic transport error (AGENTS.md §9).
    let addr = resolve(host, port).await?;

    // Build the session-health monitor before connect so the handler can
    // report the close reason from inside the session task (plan §5.4).
    let monitor = HealthMonitor::new();
    let health = monitor.handle();

    let (handler, decision) =
        HostKeyVerifyingHandler::new(host, port, known.cloned(), monitor.signal());

    let config = client::Config {
        // Keepalive (design §8.5). 25s is within the suggested 20–30s band.
        keepalive_interval: Some(Duration::from_secs(25)),
        keepalive_max: 3,
        ..client::Config::default()
    };

    // 2. Connect + verify host key. A rejection from check_server_key surfaces
    //    here as a russh error.
    let connect_fut = client::connect(Arc::new(config), addr, handler);
    let mut handle = match tokio::time::timeout(connect_timeout, connect_fut).await {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            // Inspect the decision to distinguish host-key rejection from a
            // genuine transport failure.
            return interpret_connect_error(host, port, e, decision).await;
        }
        Err(_elapsed) => {
            return Err(SshError::ConnectTimeout {
                host: host.to_string(),
                port,
            }
            .into());
        }
    };

    // 3. Authenticate.
    let result = match auth {
        SshAuth::Password(password) => handle
            .authenticate_password(username, password)
            .await
            .map_err(|e| SshError::Transport(e.to_string()))?,
        SshAuth::PrivateKey(key) => {
            let key_with_hash = PrivateKeyWithHashAlg::new(
                // Prefer SHA-256 for RSA; ignored for Ed25519.
                Arc::new(*key),
                Some(HashAlg::Sha256),
            );
            handle
                .authenticate_publickey(username, key_with_hash)
                .await
                .map_err(|e| SshError::Transport(e.to_string()))?
        }
    };

    match result {
        AuthResult::Success => Ok(ConnectOutcome::Authenticated {
            handle: Arc::new(Mutex::new(handle)),
            health,
        }),
        AuthResult::Failure { .. } => Err(SshError::AuthenticationFailed {
            user: username.to_string(),
        }
        .into()),
    }
}

async fn resolve(host: &str, port: u16) -> Result<std::net::SocketAddr, AppError> {
    use std::net::ToSocketAddrs;
    let target = format!("{host}:{port}");
    match tokio::task::spawn_blocking(move || target.to_socket_addrs()).await {
        Ok(Ok(mut addrs)) => addrs.next().ok_or_else(|| {
            SshError::DnsFailed {
                host: host.to_string(),
            }
            .into()
        }),
        Ok(Err(_)) => Err(SshError::DnsFailed {
            host: host.to_string(),
        }
        .into()),
        Err(_) => Err(SshError::DnsFailed {
            host: host.to_string(),
        }
        .into()),
    }
}

async fn interpret_connect_error(
    host: &str,
    port: u16,
    e: russh::Error,
    decision: Arc<Mutex<HostKeyDecision>>,
) -> Result<ConnectOutcome, AppError> {
    let guard = decision.lock().await;
    match &*guard {
        HostKeyDecision::NeedsConfirm(presented) => Ok(ConnectOutcome::HostKeyNeedsConfirmation(
            Box::new(presented.clone()),
        )),
        HostKeyDecision::Changed { .. } => Err(SshError::HostKeyChanged {
            host: host.to_string(),
            port,
        }
        .into()),
        _ => {
            // Genuine transport / handshake error.
            Err(SshError::Transport(e.to_string()).into())
        }
    }
}

/// Read the presented host key facts from a `NeedsConfirm` outcome so the
/// caller can build a `KnownHostRecord` after user confirmation.
pub fn presented_facts(presented: &PresentedHostKey) -> HostKeyFacts {
    HostKeyFacts {
        algorithm: presented.algorithm.clone(),
        sha256_fingerprint: presented.sha256_fingerprint.clone(),
    }
}
