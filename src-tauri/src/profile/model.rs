//! Service profile data model (docs/design-v1.md §6.1).
//!
//! Profiles are a tagged enum (`connection_type` discriminator) — never a bag
//! of nullable fields (AGENTS.md §11). Sensitive values live only in Keychain
//! and are referenced by id.

use crate::error::ProfileError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;
use uuid::Uuid;

/// Current schema version for the persisted profile store.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// SSH authentication kinds (design §7.3, V1 only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthType {
    Password,
    PrivateKey,
}

impl fmt::Display for SshAuthType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SshAuthType::Password => f.write_str("password"),
            SshAuthType::PrivateKey => f.write_str("private_key"),
        }
    }
}

/// HTTP scheme used to reach Pi Hub through a forward (design §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceScheme {
    Http,
    Https,
}

impl fmt::Display for ServiceScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceScheme::Http => f.write_str("http"),
            ServiceScheme::Https => f.write_str("https"),
        }
    }
}

/// Fields shared by every service profile (design §6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMetadata {
    pub id: Uuid,
    pub schema_version: u32,
    pub name: String,
    /// Keychain credential id for optional Pi Hub HTTP auth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_hub_credential_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Service profile — discriminated union of all V1 connection kinds.
///
/// The `#[serde(tag = "connection_type")]` discriminator mirrors the frontend
/// discriminated union (AGENTS.md §11). Each variant carries its own fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "connection_type", rename_all = "snake_case")]
pub enum ServiceProfile {
    DirectUrl(DirectUrlProfile),
    SshForward(SshForwardProfile),
}

/// Direct URL service profile (FR-005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectUrlProfile {
    #[serde(flatten)]
    pub metadata: ProfileMetadata,
    pub base_url: Url,
}

/// SSH Local Port Forward service profile (FR-006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshForwardProfile {
    #[serde(flatten)]
    pub metadata: ProfileMetadata,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub ssh_auth_type: SshAuthType,
    /// Keychain credential id for the SSH secret.
    pub ssh_credential_id: String,
    pub target_host: String,
    pub target_port: u16,
    pub service_scheme: ServiceScheme,
    pub service_base_path: String,
}

impl ServiceProfile {
    pub fn id(&self) -> Uuid {
        match self {
            ServiceProfile::DirectUrl(p) => p.metadata.id,
            ServiceProfile::SshForward(p) => p.metadata.id,
        }
    }

    pub fn metadata(&self) -> &ProfileMetadata {
        match self {
            ServiceProfile::DirectUrl(p) => &p.metadata,
            ServiceProfile::SshForward(p) => &p.metadata,
        }
    }

    pub fn metadata_mut(&mut self) -> &mut ProfileMetadata {
        match self {
            ServiceProfile::DirectUrl(p) => &mut p.metadata,
            ServiceProfile::SshForward(p) => &mut p.metadata,
        }
    }

    /// Keychain credential ids referenced by this profile, used to compute
    /// reference counts when deleting/updating profiles (design §10.3).
    pub fn credential_references(&self) -> Vec<String> {
        let mut refs = Vec::new();
        match self {
            ServiceProfile::DirectUrl(p) => {
                if let Some(id) = &p.metadata.pi_hub_credential_id {
                    refs.push(id.clone());
                }
            }
            ServiceProfile::SshForward(p) => {
                refs.push(p.ssh_credential_id.clone());
                if let Some(id) = &p.metadata.pi_hub_credential_id {
                    refs.push(id.clone());
                }
            }
        }
        refs
    }

    /// Validate profile invariants (SR-007). Does not touch secrets.
    pub fn validate(&self) -> Result<(), ProfileError> {
        match self {
            ServiceProfile::DirectUrl(p) => validate_direct(p),
            ServiceProfile::SshForward(p) => validate_ssh(p),
        }
    }
}

fn ensure_name(meta: &ProfileMetadata) -> Result<(), ProfileError> {
    if meta.name.trim().is_empty() {
        return Err(ProfileError::Invalid("name must not be empty".into()));
    }
    if meta.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(ProfileError::Invalid(format!(
            "schema_version must be {CURRENT_SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

fn validate_direct(p: &DirectUrlProfile) -> Result<(), ProfileError> {
    ensure_name(&p.metadata)?;
    match p.base_url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ProfileError::Invalid(format!(
                "base_url scheme must be http/https, got {other}"
            )));
        }
    }
    if p.base_url.host_str().is_none() {
        return Err(ProfileError::Invalid("base_url must have a host".into()));
    }
    Ok(())
}

fn validate_ssh(p: &SshForwardProfile) -> Result<(), ProfileError> {
    ensure_name(&p.metadata)?;
    if p.ssh_host.trim().is_empty() {
        return Err(ProfileError::Invalid("ssh_host must not be empty".into()));
    }
    if p.ssh_username.trim().is_empty() {
        return Err(ProfileError::Invalid(
            "ssh_username must not be empty".into(),
        ));
    }
    if p.ssh_port == 0 {
        return Err(ProfileError::Invalid("ssh_port must be non-zero".into()));
    }
    if p.target_host.trim().is_empty() {
        return Err(ProfileError::Invalid(
            "target_host must not be empty".into(),
        ));
    }
    // target_host may be a hostname, IP or loopback; SR-007 only forbids it
    // being used as a *local listener* address. The local listener is always
    // 127.0.0.1 (design §8.1) and never read from profile input.
    if p.target_port == 0 {
        return Err(ProfileError::Invalid("target_port must be non-zero".into()));
    }
    if p.ssh_credential_id.trim().is_empty() {
        return Err(ProfileError::Invalid(
            "ssh_credential_id must not be empty".into(),
        ));
    }
    match p.service_scheme {
        ServiceScheme::Http | ServiceScheme::Https => {}
    }
    Ok(())
}

/// Input used to create a new profile before ids/timestamps are assigned.
#[derive(Debug, Clone)]
pub enum ProfileInput {
    DirectUrl {
        name: String,
        base_url: Url,
        pi_hub_credential_id: Option<String>,
    },
    SshForward {
        name: String,
        ssh_host: String,
        ssh_port: u16,
        ssh_username: String,
        ssh_auth_type: SshAuthType,
        ssh_credential_id: String,
        target_host: String,
        target_port: u16,
        service_scheme: ServiceScheme,
        service_base_path: String,
        pi_hub_credential_id: Option<String>,
    },
}

impl ProfileInput {
    /// Materialize the input into a concrete profile with a fresh id and
    /// timestamps (design §11, stable UUID primary key).
    pub fn into_profile(self) -> ServiceProfile {
        let now = Utc::now();
        match self {
            ProfileInput::DirectUrl {
                name,
                base_url,
                pi_hub_credential_id,
            } => ServiceProfile::DirectUrl(DirectUrlProfile {
                metadata: ProfileMetadata {
                    id: Uuid::new_v4(),
                    schema_version: CURRENT_SCHEMA_VERSION,
                    name,
                    pi_hub_credential_id,
                    created_at: now,
                    updated_at: now,
                },
                base_url,
            }),
            ProfileInput::SshForward {
                name,
                ssh_host,
                ssh_port,
                ssh_username,
                ssh_auth_type,
                ssh_credential_id,
                target_host,
                target_port,
                service_scheme,
                service_base_path,
                pi_hub_credential_id,
            } => ServiceProfile::SshForward(SshForwardProfile {
                metadata: ProfileMetadata {
                    id: Uuid::new_v4(),
                    schema_version: CURRENT_SCHEMA_VERSION,
                    name,
                    pi_hub_credential_id,
                    created_at: now,
                    updated_at: now,
                },
                ssh_host,
                ssh_port,
                ssh_username,
                ssh_auth_type,
                ssh_credential_id,
                target_host,
                target_port,
                service_scheme,
                service_base_path,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_input() -> ProfileInput {
        ProfileInput::SshForward {
            name: "VPS".into(),
            ssh_host: "vps.example.com".into(),
            ssh_port: 22,
            ssh_username: "ubuntu".into(),
            ssh_auth_type: SshAuthType::PrivateKey,
            ssh_credential_id: "cred-1".into(),
            target_host: "127.0.0.1".into(),
            target_port: 30142,
            service_scheme: ServiceScheme::Http,
            service_base_path: "/".into(),
            pi_hub_credential_id: None,
        }
    }

    #[test]
    fn direct_profile_validates() {
        let p = ProfileInput::DirectUrl {
            name: "Cloud".into(),
            base_url: Url::parse("https://pi.example.com").unwrap(),
            pi_hub_credential_id: None,
        }
        .into_profile();
        p.validate().expect("valid direct profile");
    }

    #[test]
    fn rejects_non_http_scheme() {
        let mut p = ProfileInput::DirectUrl {
            name: "Cloud".into(),
            base_url: Url::parse("https://pi.example.com").unwrap(),
            pi_hub_credential_id: None,
        }
        .into_profile();
        if let ServiceProfile::DirectUrl(d) = &mut p {
            d.base_url = Url::parse("ftp://example.com").unwrap();
        }
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_empty_ssh_username() {
        let mut input = ssh_input();
        if let ProfileInput::SshForward { ssh_username, .. } = &mut input {
            *ssh_username = "  ".into();
        }
        let p = input.into_profile();
        assert!(matches!(p.validate(), Err(ProfileError::Invalid(_))));
    }

    #[test]
    fn rejects_zero_port() {
        let mut input = ssh_input();
        if let ProfileInput::SshForward { ssh_port, .. } = &mut input {
            *ssh_port = 0;
        }
        let p = input.into_profile();
        assert!(p.validate().is_err());
    }

    #[test]
    fn ssh_profile_materializes_with_stable_uuid_and_timestamps() {
        let p = ssh_input().into_profile();
        let m = p.metadata();
        assert_eq!(m.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(m.created_at <= m.updated_at);
    }

    #[test]
    fn credential_references_include_ssh_and_optional_pi_hub() {
        let input = ProfileInput::SshForward {
            name: "v".into(),
            ssh_host: "h".into(),
            ssh_port: 22,
            ssh_username: "u".into(),
            ssh_auth_type: SshAuthType::Password,
            ssh_credential_id: "ssh-cred".into(),
            target_host: "127.0.0.1".into(),
            target_port: 30142,
            service_scheme: ServiceScheme::Http,
            service_base_path: "/".into(),
            pi_hub_credential_id: Some("pi-cred".into()),
        }
        .into_profile();
        let refs = input.credential_references();
        assert!(refs.contains(&"ssh-cred".to_string()));
        assert!(refs.contains(&"pi-cred".to_string()));
    }

    #[test]
    fn serde_round_trips_with_snake_case_discriminator() {
        let p = ssh_input().into_profile();
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.contains("\"connection_type\":\"ssh_forward\""),
            "{json}"
        );
        assert!(json.contains("\"ssh_auth_type\":\"private_key\""));
        let back: ServiceProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
