//! SSH Forward provider (docs/design-v1.md §7.3, §8, FR-006).
//!
//! Flow: load profile → load credential → decode key (if any) → connect +
//! strict host-key verify → authenticate → bind loopback listener → start
//! accept loop → return loopback effective URL.
//!
//! Host-key confirmation is surfaced, never auto-accepted (AGENTS.md §6.2).

use crate::connection::diagnostics::ConnectionDiagnostics;
use crate::connection::provider::{
    ConnectContext, ConnectOutcome, ConnectionProvider, ConnectionResources, EstablishedConnection,
};
use crate::credential::{CredentialId, CredentialKind, CredentialStore};
use crate::error::{AppError, ProfileError, SshError};
use crate::profile::model::{ServiceProfile, SshAuthType};
use crate::ssh::client::{self, ConnectOutcome as SshConnectOutcome, SshAuth};
use crate::ssh::forward::{self, ForwardTarget};
use async_trait::async_trait;
use std::sync::Arc;
use url::Url;

pub struct SshForwardProvider;

#[async_trait]
impl ConnectionProvider for SshForwardProvider {
    async fn connect(
        &self,
        profile: &ServiceProfile,
        context: &ConnectContext,
    ) -> Result<ConnectOutcome, AppError> {
        let p = match profile {
            ServiceProfile::SshForward(p) => p,
            _ => {
                return Err(ProfileError::Invalid(
                    "SshForwardProvider received a non-ssh profile".into(),
                )
                .into());
            }
        };

        // Seed diagnostics with non-sensitive context (FR-016). No secrets.
        {
            let mut d = context.diagnostics.lock().await;
            *d = ConnectionDiagnostics::new()
                .with_stage("validating")
                .with_ssh(p.ssh_host.clone(), p.ssh_port)
                .with_target(p.target_host.clone(), p.target_port);
        }

        // Load the SSH secret from the credential store.
        let cred_id = CredentialId(p.ssh_credential_id.clone());
        let auth = build_auth(p.ssh_auth_type, &cred_id, &context.credentials).await?;

        // Connect + verify host key + authenticate.
        set_stage(&context.diagnostics, "connecting_ssh").await;
        let known = context.known_host.clone();
        let connect = client::connect_and_authenticate(
            &p.ssh_host,
            p.ssh_port,
            &p.ssh_username,
            known.as_ref(),
            auth,
        )
        .await?;

        let (handle, health) = match connect {
            SshConnectOutcome::Authenticated { handle, health } => (handle, health),
            SshConnectOutcome::HostKeyNeedsConfirmation(boxed) => {
                let presented = *boxed;
                set_stage(&context.diagnostics, "verifying_host_key").await;
                return Ok(ConnectOutcome::NeedsHostKeyConfirmation {
                    presented,
                    ssh_host: p.ssh_host.clone(),
                    ssh_port: p.ssh_port,
                });
            }
        };

        // Bind loopback listener and start the accept loop.
        set_stage(&context.diagnostics, "opening_forward").await;
        let target = ForwardTarget {
            host: p.target_host.clone(),
            port: p.target_port,
        };
        let forward =
            forward::start_local_forward(handle, target, context.cancellation.clone()).await?;

        // Mark the listener as started in diagnostics (non-sensitive).
        {
            let mut d = context.diagnostics.lock().await;
            d.listener_started = true;
            d.stage = Some("checking_service".into());
        }

        // Build the loopback effective URL the Service View will load.
        let effective_url = build_effective_url(
            p.service_scheme,
            forward.local_addr.port(),
            &p.service_base_path,
        );

        Ok(ConnectOutcome::Established(EstablishedConnection {
            effective_url,
            resources: ConnectionResources {
                forward: Some(forward),
                health: Some(health),
                cancellation: context.cancellation.clone(),
            },
        }))
    }
}

async fn build_auth(
    auth_type: SshAuthType,
    cred_id: &CredentialId,
    store: &Arc<dyn CredentialStore>,
) -> Result<SshAuth, AppError> {
    match auth_type {
        SshAuthType::Password => {
            let secret = store
                .get(cred_id, CredentialKind::SshPassword)
                .await
                .map_err(AppError::from)?;
            let bytes = secret.into_secret();
            let password = String::from_utf8(bytes).map_err(|_| SshError::PrivateKeyInvalid)?;
            Ok(SshAuth::Password(password))
        }
        SshAuthType::PrivateKey => {
            // Key + optional passphrase are stored as separate kinds.
            let key_secret = store
                .get(cred_id, CredentialKind::SshPrivateKey)
                .await
                .map_err(AppError::from)?;
            let pem = String::from_utf8(key_secret.into_secret())
                .map_err(|_| SshError::PrivateKeyInvalid)?;

            let passphrase = match store.get(cred_id, CredentialKind::SshKeyPassphrase).await {
                Ok(s) => {
                    let b = s.into_secret();
                    if b.is_empty() {
                        None
                    } else {
                        Some(String::from_utf8(b).map_err(|_| SshError::PrivateKeyInvalid)?)
                    }
                }
                // No passphrase stored → key may be plaintext.
                Err(crate::error::CredentialError::NotFound) => None,
                Err(e) => return Err(AppError::from(e)),
            };

            let key = crate::ssh::key_loader::decode(&pem, passphrase.as_deref())?;
            Ok(SshAuth::PrivateKey(Box::new(key)))
        }
    }
}

fn build_effective_url(
    scheme: crate::profile::model::ServiceScheme,
    port: u16,
    base_path: &str,
) -> Url {
    let scheme_str = match scheme {
        crate::profile::model::ServiceScheme::Http => "http",
        crate::profile::model::ServiceScheme::Https => "https",
    };
    // Always loopback by design (AGENTS.md §6.3). The random port is the one
    // allocated by the OS for the local listener.
    let path = if base_path.is_empty() { "/" } else { base_path };
    Url::parse(&format!("{scheme_str}://127.0.0.1:{port}{path}"))
        .expect("effective url is well-formed by construction")
}

async fn set_stage(d: &Arc<tokio::sync::Mutex<ConnectionDiagnostics>>, stage: &str) {
    d.lock().await.stage = Some(stage.to_string());
}

// Helper trait to fluently set non-sensitive diagnostics fields.
trait DiagExt {
    fn with_ssh(self, host: String, port: u16) -> Self;
    fn with_target(self, host: String, port: u16) -> Self;
}
impl DiagExt for ConnectionDiagnostics {
    fn with_ssh(mut self, host: String, port: u16) -> Self {
        self.ssh_host = Some(host);
        self.ssh_port = Some(port);
        self
    }
    fn with_target(mut self, host: String, port: u16) -> Self {
        self.target_host = Some(host);
        self.target_port = Some(port);
        self
    }
}
