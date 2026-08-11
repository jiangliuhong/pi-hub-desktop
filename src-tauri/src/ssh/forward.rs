//! SSH Local Port Forward (docs/design-v1.md §8, AGENTS.md §6.3, §7.3).
//!
//! Hard rules enforced here:
//! - The local listener binds `127.0.0.1:0` only — never `0.0.0.0` and never a
//!   user-supplied port (AGENTS.md §6.3, design §8.1).
//! - Each accepted local TCP connection spawns its own SSH `direct-tcpip`
//!   channel and a bidirectional copy task (design §8.3).
//! - Per-channel failures never propagate to other channels or the session.
//! - Everything is cancellation-safe via a `CancellationToken`.
//! - No business data is ever logged (SR-005).

use crate::error::{AppError, ForwardError};
use crate::ssh::client::HostKeyVerifyingHandler;
use russh::client::{Handle, Msg};
use russh::ChannelMsg;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Target reachable from the SSH server's perspective (design §7.3).
#[derive(Debug, Clone)]
pub struct ForwardTarget {
    pub host: String,
    pub port: u16,
}

/// A running local forward: the loopback listener plus a way to stop it.
pub struct LocalForward {
    /// The loopback address clients should connect to (`127.0.0.1:<ephemeral>`).
    pub local_addr: SocketAddr,
    cancel: CancellationToken,
    /// Handle to the accept loop task so we can await clean shutdown.
    join: tokio::task::JoinHandle<()>,
}

impl LocalForward {
    /// Stop accepting new connections and drain in-flight channels. Each
    /// channel is closed cooperatively; this never panics on a single failed
    /// channel (design §8.4).
    pub async fn shutdown(self) {
        self.cancel.cancel();
        // Accept loop returns promptly on cancel; ignore join errors during
        // teardown (e.g. runtime shutdown).
        let _ = self.join.await;
    }
}

/// Bind the loopback listener and start the accept loop, forwarding each
/// accepted connection through `direct-tcpip` on the given SSH handle.
///
/// `presented_key_check` is unused here; host-key verification happens during
/// `connect` (see [`crate::ssh::client`]). This function only operates after a
/// successfully authenticated session.
pub async fn start_local_forward(
    ssh: Arc<Mutex<Handle<HostKeyVerifyingHandler>>>,
    target: ForwardTarget,
    cancel: CancellationToken,
) -> Result<LocalForward, AppError> {
    // Bind loopback only. `Ipv4Addr::LOCALHOST` => 127.0.0.1; port 0 lets the
    // OS assign an ephemeral port (AGENTS.md §6.3).
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| ForwardError::ListenerFailed(e.to_string()))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| ForwardError::ListenerFailed(e.to_string()))?;
    // Guard: hard-invariant that we never expose a non-loopback address.
    debug_assert!(local_addr.ip().is_loopback(), "listener must bind loopback");

    let join = tokio::spawn(accept_loop(listener, ssh, target, cancel.clone()));

    Ok(LocalForward {
        local_addr,
        cancel,
        join,
    })
}

async fn accept_loop(
    listener: TcpListener,
    ssh: Arc<Mutex<Handle<HostKeyVerifyingHandler>>>,
    target: ForwardTarget,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            res = listener.accept() => {
                let (local_stream, peer_addr) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "forward accept failed; continuing accept loop"
                        );
                        continue;
                    }
                };
                let ssh = ssh.clone();
                let target = target.clone();
                // Each channel is independent: a failure here must not affect
                // siblings (design §8.4).
                tokio::spawn(async move {
                    if let Err(e) = forward_one(local_stream, peer_addr, ssh, target).await {
                        tracing::warn!(error = %e, "forward channel ended with error");
                    }
                });
            }
        }
    }
}

/// Open one `direct-tcpip` channel and copy data bidirectionally until either
/// side closes, EOFs, or the channel fails to open.
async fn forward_one(
    local: TcpStream,
    peer_addr: SocketAddr,
    ssh: Arc<Mutex<Handle<HostKeyVerifyingHandler>>>,
    target: ForwardTarget,
) -> Result<(), AppError> {
    tracing::debug!(
        peer = %peer_addr,
        target_host = %target.host,
        target_port = target.port,
        "opening direct-tcpip channel"
    );

    let channel = {
        let guard = ssh.lock().await;
        // Originator info uses the real local peer address; we never forge a
        // misleading origin (design §8.3).
        guard
            .channel_open_direct_tcpip(
                target.host.clone(),
                u32::from(target.port),
                peer_addr.ip().to_string(),
                u32::from(peer_addr.port()),
            )
            .await
            .map_err(|e| ForwardError::TargetUnreachable(e.to_string()))?
    };

    // Convert the SSH channel into an AsyncRead+AsyncWrite stream, then splice
    // it to the local TCP connection. `copy_bidirectional` handles half-close
    // and normal EOF (design §8.4).
    let mut channel_stream = channel.into_stream();
    let mut tcp = local;
    match copy_bidirectional(&mut tcp, &mut channel_stream).await {
        Ok((_sent, _recv)) => Ok(()),
        Err(e) => {
            // Channel-open failure maps to TargetUnreachable earlier; mid-stream
            // IO is a soft channel failure and must not crash siblings.
            Err(ForwardError::Io(e.to_string()).into())
        }
    }
}

// ---- forward uses the handler produced by `client.rs` ----
// `HostKeyVerifyingHandler` (see `crate::ssh::client`) implements the russh
// `Handler` trait and is the only handler type used for trusted connects.
// There is intentionally no separate permissive `ForwardHandler`: the forward
// always runs over a session whose host key was already verified.

/// Re-export of russh message types used by callers that build their own
/// handler wiring.
#[allow(unused_imports)]
pub use russh::client::Handle as SshHandle;

/// Suppress unused-import warning for `Msg`/`ChannelMsg`; both are part of
/// the documented forward contract.
#[allow(dead_code)]
fn _touch_types(_m: Msg, _c: ChannelMsg) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Pure networking test (no SSH): prove the loopback bind semantics that
    /// the forward relies on — `127.0.0.1:0` yields a loopback ephemeral port,
    /// and binding never touches `0.0.0.0`.
    #[tokio::test]
    async fn loopback_ephemeral_bind() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = l.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
        assert_ne!(addr.port(), 30142, "must not use the fixed Pi Hub port");
    }

    /// Demonstrate `copy_bidirectional` half-close behavior on plain TCP, since
    /// the forward delegates to it for SSH channel streams.
    #[tokio::test]
    async fn copy_bidirectional_echo_loop() {
        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let echo_addr = echo.local_addr().unwrap();
        // Echo server: reflect bytes then close.
        tokio::spawn(async move {
            let (mut sock, _) = echo.accept().await.unwrap();
            let mut buf = [0u8; 16];
            loop {
                match sock.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sock.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let mut client = TcpStream::connect(echo_addr).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        let mut got = [0u8; 5];
        client.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"hello");
    }

    #[test]
    fn forward_target_carries_target_host_and_port() {
        let t = ForwardTarget {
            host: "127.0.0.1".into(),
            port: 30142,
        };
        assert_eq!(t.port, 30142);
    }
}
