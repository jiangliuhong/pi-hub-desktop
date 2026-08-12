//! SSH session health monitoring (plan-remote-pi-hub-performance §5.4).
//!
//! The connection reliability feature needs to observe when an already
//! established SSH transport ends — whether the remote sent an explicit
//! disconnect, keepalive probes went unanswered past the threshold, or the
//! underlying socket died. russh enforces keepalive internally
//! (`keepalive_interval` / `keepalive_max` in `client.rs`) and terminates the
//! session task once the threshold is crossed, but without this module nothing
//! in the application is notified: the `Handle` simply goes dead and every
//! subsequent `direct-tcpip` open fails per-channel.
//!
//! Detection is done via the `Handler::disconnected` callback, which russh
//! invokes for **both** clean closes (`ReceivedDisconnect`) and error-driven
//! closes (including `Error::KeepaliveTimeout`). The handler writes a
//! classified, non-sensitive [`SessionCloseReason`] into a `tokio::watch`
//! channel; [`HealthHandle`] exposes the receive end so the connection
//! supervisor can await session loss without holding the SSH handle lock.
//!
//! Nothing here records business data: the reason is a stable enum, never the
//! raw russh error string or any SSH payload.

use crate::error::SessionCloseReason;
use russh::client::DisconnectReason;
use tokio::sync::watch;

/// Shared, cloneable sender pair the SSH handler writes to from inside the
/// session task. Created by [`HealthMonitor::new`] and injected into the
/// handler before connect.
#[derive(Clone)]
pub(crate) struct HealthSignal {
    tx: watch::Sender<Option<SessionCloseReason>>,
}

impl HealthSignal {
    /// Record that the session ended. Called from `Handler::disconnected`.
    /// Idempotent: the first reason wins; subsequent calls are no-ops (the
    /// session is already dead).
    pub(crate) fn report_close(&self, reason: SessionCloseReason) {
        // `send` only fails if all receivers were dropped. The supervisor
        // holds a receiver for the connection's lifetime, so this is a sanity
        // guard; if the receiver is already gone there is nobody to notify.
        let _ = self.tx.send_if_modified(|slot| {
            if slot.is_none() {
                *slot = Some(reason);
                true
            } else {
                false
            }
        });
    }
}

/// Constructor + consumer side of the session-health channel.
///
/// `HealthMonitor` is created before `russh::client::connect`. Its
/// [`HealthSignal`] is handed to the handler; its [`HealthHandle`] is returned
/// to the caller alongside the authenticated SSH handle.
pub(crate) struct HealthMonitor {
    signal: HealthSignal,
    rx: watch::Receiver<Option<SessionCloseReason>>,
}

impl HealthMonitor {
    pub(crate) fn new() -> Self {
        let (tx, rx) = watch::channel(None);
        HealthMonitor {
            signal: HealthSignal { tx },
            rx,
        }
    }

    /// The cloneable write end, injected into the handler.
    pub(crate) fn signal(&self) -> HealthSignal {
        self.signal.clone()
    }

    /// The read end, handed to the connection supervisor.
    pub(crate) fn handle(&self) -> HealthHandle {
        HealthHandle {
            rx: self.rx.clone(),
        }
    }
}

/// Read-only view of session health for the connection supervisor.
///
/// Cheap to clone (just a `watch::Receiver`). The supervisor holds one for the
/// connection's lifetime and awaits [`HealthHandle::closed`] in a
/// `select!` against its cancellation token.
///
/// Exposed as a field of `ConnectionResources` / `ConnectOutcome` so it is
/// `pub`; the module itself is `pub(crate)` so the type can't be named outside
/// the crate.
#[derive(Clone)]
pub struct HealthHandle {
    rx: watch::Receiver<Option<SessionCloseReason>>,
}

impl HealthHandle {
    /// Returns `true` if the session has already ended. Non-async, lock-free.
    pub fn is_closed(&self) -> bool {
        self.rx.borrow().is_some()
    }

    /// The classified close reason, if the session has ended.
    pub fn close_reason(&self) -> Option<SessionCloseReason> {
        *self.rx.borrow()
    }

    /// Resolves when the session ends, returning the reason. Never resolves
    /// while the session is alive. Cancel-safe to use in `tokio::select!`.
    ///
    /// If the session was already closed before the first await, resolves
    /// immediately on the first poll.
    pub async fn closed(&mut self) -> SessionCloseReason {
        if let Some(reason) = *self.rx.borrow() {
            return reason;
        }
        // `changed()` resolves when a sender writes a new value. Because
        // `report_close` only ever transitions None → Some(once), the first
        // change is the close.
        loop {
            if self.rx.changed().await.is_err() {
                // All senders dropped without reporting a reason. Treat as an
                // unknown close so the supervisor still triggers reconnect.
                return SessionCloseReason::Unknown;
            }
            if let Some(reason) = *self.rx.borrow() {
                return reason;
            }
        }
    }
}

/// Classify a russh `DisconnectReason` into a non-sensitive
/// [`SessionCloseReason`]. Used by the handler override in `client.rs`.
///
/// Keepalive timeout is distinguished so diagnostics/reconnect logic can tell
/// "server stopped responding" from "socket died" — the two have different
/// remediation implications (plan §5.4 / §5.6).
pub(crate) fn classify_disconnect(reason: &DisconnectReason<russh::Error>) -> SessionCloseReason {
    match reason {
        DisconnectReason::ReceivedDisconnect(_) => SessionCloseReason::RemoteDisconnect,
        DisconnectReason::Error(e) => match e {
            russh::Error::KeepaliveTimeout => SessionCloseReason::KeepaliveTimeout,
            russh::Error::HUP
            | russh::Error::IO(_)
            | russh::Error::ConnectionTimeout
            | russh::Error::Disconnect => SessionCloseReason::NetworkError,
            _ => SessionCloseReason::Unknown,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn first_close_reason_wins() {
        let monitor = HealthMonitor::new();
        let signal = monitor.signal();
        let handle = monitor.handle();
        assert!(!handle.is_closed());
        assert_eq!(handle.close_reason(), None);

        signal.report_close(SessionCloseReason::KeepaliveTimeout);
        // A second report must not overwrite the first.
        signal.report_close(SessionCloseReason::NetworkError);

        assert!(handle.is_closed());
        assert_eq!(
            handle.close_reason(),
            Some(SessionCloseReason::KeepaliveTimeout)
        );
    }

    #[tokio::test]
    async fn closed_resolves_after_report() {
        let monitor = HealthMonitor::new();
        let signal = monitor.signal();
        let mut handle = monitor.handle();

        // Report from a separate task after a short delay.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            signal.report_close(SessionCloseReason::RemoteDisconnect);
        });

        let reason = tokio::time::timeout(Duration::from_secs(1), handle.closed())
            .await
            .expect("closed() should resolve after report");
        assert_eq!(reason, SessionCloseReason::RemoteDisconnect);
    }

    #[tokio::test]
    async fn closed_resolves_immediately_if_already_closed() {
        let monitor = HealthMonitor::new();
        monitor
            .signal()
            .report_close(SessionCloseReason::NetworkError);
        let mut handle = monitor.handle();
        let reason = handle.closed().await;
        assert_eq!(reason, SessionCloseReason::NetworkError);
    }

    #[test]
    fn classify_clean_disconnect() {
        let reason = DisconnectReason::<russh::Error>::ReceivedDisconnect(
            russh::client::RemoteDisconnectInfo {
                reason_code: russh::Disconnect::ByApplication,
                message: String::new(),
                lang_tag: String::new(),
            },
        );
        assert_eq!(
            classify_disconnect(&reason),
            SessionCloseReason::RemoteDisconnect
        );
    }

    #[test]
    fn classify_keepalive_timeout() {
        let reason = DisconnectReason::<russh::Error>::Error(russh::Error::KeepaliveTimeout);
        assert_eq!(
            classify_disconnect(&reason),
            SessionCloseReason::KeepaliveTimeout
        );
    }

    #[test]
    fn classify_network_errors() {
        assert_eq!(
            classify_disconnect(&DisconnectReason::Error(russh::Error::HUP)),
            SessionCloseReason::NetworkError
        );
        assert_eq!(
            classify_disconnect(&DisconnectReason::Error(russh::Error::Disconnect)),
            SessionCloseReason::NetworkError
        );
        let io_err = russh::Error::IO(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "rst",
        ));
        assert_eq!(
            classify_disconnect(&DisconnectReason::Error(io_err)),
            SessionCloseReason::NetworkError
        );
    }
}
