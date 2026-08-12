//! Direct URL provider (docs/design-v1.md §7.2, FR-005).
//!
//! Flow: validate URL → validate scheme/TLS policy → return the URL. HTTPS is
//! fully supported; HTTP is allowed (loopback / trusted network) but the *UI*
//! is responsible for the one-time plaintext warning before this is called
//! (FR-005). TLS errors are never ignorable (AGENTS.md §6.5).
//!
//! V1 does not enforce a reachability probe as the sole success criterion: a
//! Pi Hub that returns 401 is still "reachable, needs auth" (design §17.2).

use crate::connection::provider::{
    ConnectContext, ConnectOutcome, ConnectionProvider, ConnectionResources, EstablishedConnection,
};
use crate::error::{AppError, ProfileError, ServiceError};
use crate::profile::model::ServiceProfile;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

/// Direct URL provider: no SSH, no local listener.
pub struct DirectUrlProvider;

#[async_trait]
impl ConnectionProvider for DirectUrlProvider {
    async fn connect(
        &self,
        profile: &ServiceProfile,
        context: &ConnectContext,
    ) -> Result<ConnectOutcome, AppError> {
        let base = match profile {
            ServiceProfile::DirectUrl(p) => &p.base_url,
            _ => {
                return Err(ProfileError::Invalid(
                    "DirectUrlProvider received a non-direct profile".into(),
                )
                .into());
            }
        };

        {
            let mut d = context.diagnostics.lock().await;
            *d = d.clone().with_stage("validating");
        }

        match base.scheme() {
            "https" | "http" => {}
            other => {
                return Err(ServiceError::Http(format!("unsupported scheme {other}")).into());
            }
        }
        if base.host_str().is_none() {
            return Err(ProfileError::Invalid("base_url missing host".into()).into());
        }

        // Honour cancellation between validation and success.
        if context.cancellation.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        Ok(ConnectOutcome::Established(EstablishedConnection {
            effective_url: base.clone(),
            resources: ConnectionResources {
                forward: None,
                health: None,
                cancellation: CancellationToken::new(),
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::diagnostics::ConnectionDiagnostics;
    use crate::credential::in_memory::InMemoryCredentialStore;
    use crate::profile::model::ProfileInput;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn ctx() -> ConnectContext {
        ConnectContext {
            cancellation: CancellationToken::new(),
            known_host: None,
            credentials: Arc::new(InMemoryCredentialStore::new()),
            diagnostics: Arc::new(Mutex::new(ConnectionDiagnostics::new())),
        }
    }

    #[tokio::test]
    async fn returns_effective_url_for_https() {
        let p = ProfileInput::DirectUrl {
            name: "c".into(),
            base_url: url::Url::parse("https://pi.example.com").unwrap(),
            pi_hub_credential_id: None,
        }
        .into_profile();
        let outcome = DirectUrlProvider.connect(&p, &ctx()).await.unwrap();
        match outcome {
            ConnectOutcome::Established(e) => {
                assert_eq!(e.effective_url.as_str(), "https://pi.example.com/");
                assert!(!e.resources.listener_started());
            }
            _ => panic!("expected established"),
        }
    }

    #[tokio::test]
    async fn rejects_non_direct_profile() {
        let p = ProfileInput::SshForward {
            name: "s".into(),
            ssh_host: "h".into(),
            ssh_port: 22,
            ssh_username: "u".into(),
            ssh_auth_type: crate::profile::model::SshAuthType::Password,
            ssh_credential_id: "c".into(),
            target_host: "127.0.0.1".into(),
            target_port: 30142,
            service_scheme: crate::profile::model::ServiceScheme::Http,
            service_base_path: "/".into(),
            pi_hub_credential_id: None,
        }
        .into_profile();
        assert!(DirectUrlProvider.connect(&p, &ctx()).await.is_err());
    }

    #[tokio::test]
    async fn honours_cancellation() {
        let p = ProfileInput::DirectUrl {
            name: "c".into(),
            base_url: url::Url::parse("https://pi.example.com").unwrap(),
            pi_hub_credential_id: None,
        }
        .into_profile();
        let c = ctx();
        c.cancellation.cancel();
        assert!(matches!(
            DirectUrlProvider.connect(&p, &c).await,
            Err(AppError::Cancelled)
        ));
    }
}
