//! Connection provider abstraction (docs/design-v1.md §7).
//!
//! Connection kinds implement a single `ConnectionProvider` trait. The UI
//! never touches SSH directly — it drives providers through the
//! ConnectionManager (AGENTS.md §5.1, §5.3). Future Relay support adds a new
//! provider without changing the UI core (NFR-005).

use crate::connection::diagnostics::ConnectionDiagnostics;
use crate::credential::CredentialStore;
use crate::error::AppError;
use crate::profile::model::ServiceProfile;
use crate::ssh::client::PresentedHostKey;
use crate::ssh::forward::LocalForward;
use crate::ssh::health::HealthHandle;
use crate::ssh::host_key::KnownHostRecord;
use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Inputs handed to a provider for a connect attempt.
pub struct ConnectContext {
    pub cancellation: CancellationToken,
    /// Previously confirmed host key for the SSH endpoint, if any (None on a
    /// brand-new endpoint → provider returns `NeedsHostKeyConfirmation`).
    pub known_host: Option<KnownHostRecord>,
    /// Credential store for loading SSH secrets / Pi Hub password.
    pub credentials: Arc<dyn CredentialStore>,
    /// Populated incrementally as the attempt progresses; surfaced to the UI.
    pub diagnostics: Arc<tokio::sync::Mutex<ConnectionDiagnostics>>,
}

/// Outcome of a connect attempt.
pub enum ConnectOutcome {
    /// The service is reachable; open the Service View at `effective_url`.
    Established(EstablishedConnection),
    /// SSH endpoint not yet trusted. The caller must confirm `presented`,
    /// persist a `KnownHostRecord`, then retry the connect.
    NeedsHostKeyConfirmation {
        presented: PresentedHostKey,
        ssh_host: String,
        ssh_port: u16,
    },
}

/// A live connection plus its effective URL and owned resources.
pub struct EstablishedConnection {
    pub effective_url: Url,
    pub resources: ConnectionResources,
}

/// RAII handle over connection resources. Dropping cancels the token; the
/// explicit `shutdown` performs an orderly close (design §7.1, §8.4).
pub struct ConnectionResources {
    /// SSH forward (loopback listener + channels), if any. `None` for direct.
    pub forward: Option<LocalForward>,
    /// SSH session-health monitor, if any. `None` for direct. The supervisor
    /// clones this and awaits session loss to trigger reconnect (plan §5.4).
    pub health: Option<HealthHandle>,
    pub cancellation: CancellationToken,
}

impl ConnectionResources {
    /// Orderly shutdown: cancel tasks then drain the forward listener.
    pub async fn shutdown(self) {
        self.cancellation.cancel();
        if let Some(fwd) = self.forward {
            fwd.shutdown().await;
        }
        // The health handle is a read-only `watch::Receiver`; its sender is
        // dropped when the SSH session task ends. Nothing to await here.
    }

    /// True when the loopback listener has been bound (used in diagnostics).
    pub fn listener_started(&self) -> bool {
        self.forward.is_some()
    }
}

/// Implemented by each connection kind.
#[async_trait]
pub trait ConnectionProvider: Send + Sync {
    /// Attempt to establish a connection for `profile`.
    async fn connect(
        &self,
        profile: &ServiceProfile,
        context: &ConnectContext,
    ) -> Result<ConnectOutcome, AppError>;
}
