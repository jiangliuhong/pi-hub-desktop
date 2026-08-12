//! Post-install verification (docs/requirements-v3.md V3-SR-007; design §17.4).
//!
//! Runs **before** activation, on the staging copy. If any check fails, the
//! staging dir is discarded and the active installation is left untouched
//! (design §11.1). Verification never trusts file names alone: package
//! identity, exact version, the bin entry, the production build (Pi Hub), and a
//! live `--version` run must all agree.

use crate::error::PackageManagementError;
use crate::local_runtime::detector::CommandRunner;
use crate::package_management::model::{bin_name, package_name, ProductId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const SHORT_TIMEOUT: Duration = Duration::from_secs(15);

/// A verified staging install, safe to activate (design §11.1).
#[derive(Debug, Clone)]
pub struct VerifiedInstall {
    pub product: ProductId,
    pub version: semver::Version,
    pub package_root: PathBuf,
    pub entrypoint: PathBuf,
}

/// The verifier contract (DI for tests).
#[async_trait::async_trait]
pub trait PostInstallVerifier: Send + Sync {
    async fn verify(
        &self,
        product: ProductId,
        expected_version: semver::Version,
        staging_dir: &Path,
        node_executable: &Path,
    ) -> Result<VerifiedInstall, PackageManagementError>;
}

/// Production verifier.
pub struct DefaultPostInstallVerifier {
    runner: Arc<dyn CommandRunner>,
}

impl DefaultPostInstallVerifier {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        DefaultPostInstallVerifier { runner }
    }
    pub fn with_default_runner() -> Self {
        Self::new(Arc::new(crate::local_runtime::detector::TokioCommandRunner))
    }
}

#[async_trait::async_trait]
impl PostInstallVerifier for DefaultPostInstallVerifier {
    async fn verify(
        &self,
        product: ProductId,
        expected_version: semver::Version,
        staging_dir: &Path,
        node_executable: &Path,
    ) -> Result<VerifiedInstall, PackageManagementError> {
        let pkg = package_name(product);
        let pkg_root = canonicalize_required(&staging_dir.join("node_modules").join(pkg))?;
        if !pkg_root.is_dir() {
            return Err(VerificationError::new(
                product,
                "package root not found in staging",
            ));
        }

        // 1. package.json identity + version.
        let identity = read_package_identity(&pkg_root, product)?;
        if identity.name != pkg {
            return Err(VerificationError::new(product, "package name mismatch"));
        }
        let parsed = semver::Version::parse(&identity.version)
            .map_err(|_| VerificationError::new(product, "version not semver"))?;
        if parsed != expected_version {
            return Err(VerificationError::new(
                product,
                format!("version mismatch: expected {expected_version}, got {parsed}"),
            ));
        }

        // 2. bin entry resolves to a real file.
        let entry_rel = identity
            .bin
            .ok_or_else(|| VerificationError::new(product, "no bin entry"))?;
        let entry_abs = pkg_root.join(&entry_rel);
        let entrypoint = canonicalize_required(&entry_abs)?;
        if !entrypoint.is_file() {
            return Err(VerificationError::new(product, "bin entry not a file"));
        }

        // 3. Pi Hub: production build marker (.next).
        if product == ProductId::PiHub && !pkg_root.join(".next").is_dir() {
            return Err(VerificationError::new(
                product,
                "missing .next production build",
            ));
        }

        // 4. Live identity run with the candidate node.
        let live = run_version(node_executable, &entrypoint, product, &self.runner).await?;
        if product == ProductId::PiHub {
            // pi-hub --version --json must report the right name + version.
            if live.name.as_deref() != Some(pkg) {
                return Err(VerificationError::new(product, "live name mismatch"));
            }
            let live_v = semver::Version::parse(&live.version)
                .map_err(|_| VerificationError::new(product, "live version not semver"))?;
            if live_v != expected_version {
                return Err(VerificationError::new(product, "live version mismatch"));
            }
        } else {
            // pi --version prints "pi <version>"; ensure the token matches.
            let parsed = semver::Version::parse(&live.version)
                .map_err(|_| VerificationError::new(product, "live version not semver"))?;
            if parsed != expected_version {
                return Err(VerificationError::new(product, "live version mismatch"));
            }
        }

        Ok(VerifiedInstall {
            product,
            version: parsed,
            package_root: pkg_root,
            entrypoint,
        })
    }
}

/// Parsed package.json identity fields.
struct PackageIdentity {
    name: String,
    version: String,
    #[allow(dead_code)]
    engine: Option<String>,
    bin: Option<String>,
}

fn read_package_identity(
    pkg_root: &Path,
    product: ProductId,
) -> Result<PackageIdentity, PackageManagementError> {
    let raw = std::fs::read_to_string(pkg_root.join("package.json"))
        .map_err(|_| VerificationError::new(product, "no package.json"))?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|_| VerificationError::new(product, "package.json not json"))?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| VerificationError::new(product, "no name"))?
        .to_string();
    let version = v
        .get("version")
        .and_then(|n| n.as_str())
        .ok_or_else(|| VerificationError::new(product, "no version"))?
        .to_string();
    let engine = v
        .get("engines")
        .and_then(|e| e.get("node"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    let bin = resolve_bin(&v, bin_name(product));
    Ok(PackageIdentity {
        name,
        version,
        engine,
        bin,
    })
}

/// Resolve the bin target for `bin_name` from a `bin` field that may be a
/// string or an object.
fn resolve_bin(pkg: &serde_json::Value, bin_name: &str) -> Option<String> {
    let bin = pkg.get("bin")?;
    if let Some(s) = bin.as_str() {
        // Single-bin package; the binary name is the package's bin name.
        return Some(s.to_string());
    }
    bin.as_object()
        .and_then(|m| m.get(bin_name))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

struct LiveVersion {
    name: Option<String>,
    version: String,
}

async fn run_version(
    node: &Path,
    entrypoint: &Path,
    product: ProductId,
    runner: &Arc<dyn CommandRunner>,
) -> Result<LiveVersion, PackageManagementError> {
    let out = if product == ProductId::PiHub {
        runner
            .run(
                node,
                &[entrypoint.to_string_lossy().as_ref(), "--version", "--json"],
                Some(entrypoint.parent().unwrap_or(Path::new("."))),
                SHORT_TIMEOUT,
                &[],
            )
            .await
    } else {
        runner
            .run(
                node,
                &[entrypoint.to_string_lossy().as_ref(), "--version"],
                None,
                SHORT_TIMEOUT,
                &[],
            )
            .await
    };
    let out = out.map_err(|_| VerificationError::new(product, "version run failed"))?;
    if !matches!(out.exit_code, Some(0)) {
        return Err(VerificationError::new(product, "version run non-zero"));
    }
    if product == ProductId::PiHub {
        let parsed: serde_json::Value = serde_json::from_str(out.stdout.trim())
            .map_err(|_| VerificationError::new(product, "version json invalid"))?;
        let name = parsed
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        let version = parsed
            .get("version")
            .and_then(|n| n.as_str())
            .ok_or_else(|| VerificationError::new(product, "version json no version"))?
            .to_string();
        Ok(LiveVersion { name, version })
    } else {
        // `pi --version` → "pi 0.84.0"
        let token = out
            .stdout
            .split_whitespace()
            .last()
            .ok_or_else(|| VerificationError::new(product, "empty version output"))?;
        Ok(LiveVersion {
            name: None,
            version: token.trim().to_string(),
        })
    }
}

fn canonicalize_required(p: &Path) -> Result<PathBuf, PackageManagementError> {
    std::fs::canonicalize(p).map_err(|_| {
        VerificationError::new_internal(format!("canonicalize failed: {}", p.display()))
    })
}

/// Helper constructors that map to the stable VerificationFailed code.
#[allow(non_snake_case)]
mod VerificationError {
    use super::*;

    pub(crate) fn new(product: ProductId, reason: impl Into<String>) -> PackageManagementError {
        PackageManagementError::VerificationFailed {
            product: product.api_name().into(),
            reason: reason.into(),
        }
    }

    pub(crate) fn new_internal(reason: impl Into<String>) -> PackageManagementError {
        PackageManagementError::Internal(reason.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LocalRuntimeError;
    use crate::local_runtime::detector::CommandOutput;
    use async_trait::async_trait;
    use std::path::Path;

    struct OkRunner {
        json: String,
    }
    #[async_trait]
    impl CommandRunner for OkRunner {
        async fn run(
            &self,
            _program: &Path,
            _args: &[&str],
            _cwd: Option<&Path>,
            _timeout: Duration,
            _env: &[(&str, &str)],
        ) -> Result<CommandOutput, LocalRuntimeError> {
            Ok(CommandOutput {
                exit_code: Some(0),
                stdout: self.json.clone(),
                stderr: "".into(),
            })
        }
    }

    fn plant_pi(staging: &Path, version: &str) -> PathBuf {
        let pkg = "@earendil-works/pi-coding-agent";
        let root = staging.join("node_modules").join(pkg);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::json!({
                "name": pkg,
                "version": version,
                "bin": { "pi": "./bin/pi.js" }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(root.join("bin/pi.js"), "#!/usr/bin/env node\n").unwrap();
        root
    }

    #[tokio::test]
    async fn verifies_matching_pi_install() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        plant_pi(&staging, "0.84.0");
        let node = dir.path().join("node");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        let v = DefaultPostInstallVerifier::new(Arc::new(OkRunner {
            json: "pi 0.84.0".into(),
        }));
        let res = v
            .verify(
                ProductId::Pi,
                semver::Version::new(0, 84, 0),
                &staging,
                &node,
            )
            .await
            .unwrap();
        assert_eq!(res.version, semver::Version::new(0, 84, 0));
        assert!(res.entrypoint.ends_with("bin/pi.js"));
    }

    #[tokio::test]
    async fn rejects_version_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        plant_pi(&staging, "0.83.0");
        let node = dir.path().join("node");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        let v = DefaultPostInstallVerifier::new(Arc::new(OkRunner {
            json: "pi 0.83.0".into(),
        }));
        let res = v
            .verify(
                ProductId::Pi,
                semver::Version::new(0, 84, 0),
                &staging,
                &node,
            )
            .await;
        assert!(matches!(
            res,
            Err(PackageManagementError::VerificationFailed { .. })
        ));
    }

    #[tokio::test]
    async fn rejects_pi_hub_without_next_build() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let pkg = "@jarome/pi-hub";
        let root = staging.join("node_modules").join(pkg);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::json!({
                "name": pkg,
                "version": "0.0.42",
                "bin": { "pi-hub": "./bin/pi-hub.js" }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(root.join("bin/pi-hub.js"), "#!/usr/bin/env node\n").unwrap();
        // No .next dir.
        let node = dir.path().join("node");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        let v = DefaultPostInstallVerifier::new(Arc::new(OkRunner {
            json: r#"{"name":"@jarome/pi-hub","version":"0.0.42"}"#.into(),
        }));
        let res = v
            .verify(
                ProductId::PiHub,
                semver::Version::new(0, 0, 42),
                &staging,
                &node,
            )
            .await;
        assert!(matches!(
            res,
            Err(PackageManagementError::VerificationFailed { .. })
        ));
    }

    #[tokio::test]
    async fn verifies_pi_hub_with_next_build() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let pkg = "@jarome/pi-hub";
        let root = staging.join("node_modules").join(pkg);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join(".next")).unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::json!({
                "name": pkg,
                "version": "0.0.42",
                "bin": { "pi-hub": "./bin/pi-hub.js" }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(root.join("bin/pi-hub.js"), "#!/usr/bin/env node\n").unwrap();
        let node = dir.path().join("node");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        let v = DefaultPostInstallVerifier::new(Arc::new(OkRunner {
            json: r#"{"name":"@jarome/pi-hub","version":"0.0.42"}"#.into(),
        }));
        let res = v
            .verify(
                ProductId::PiHub,
                semver::Version::new(0, 0, 42),
                &staging,
                &node,
            )
            .await
            .unwrap();
        assert_eq!(res.version, semver::Version::new(0, 0, 42));
    }
}
