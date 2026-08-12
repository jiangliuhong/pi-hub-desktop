//! npm toolchain detection (docs/requirements-v3.md §8.3, §10;
//! design §10).
//!
//! Locates an `npm-cli.js` paired with a verified Node.js, then validates it
//! by running `<node> <npm-cli.js> --version`. Never invokes a shell, never
//! uses `which`/`command -v` (V3-SR-001). The Node → npm pairing prefers the
//! same install prefix (design §10).

use crate::error::PackageManagementError;
use crate::local_runtime::detector::{CommandRunner, TokioCommandRunner};
use crate::local_runtime::model::{InstallationSource, NodeInstallation};
use crate::package_management::model::ProductId;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const SHORT_TIMEOUT: Duration = Duration::from_secs(8);

/// A verified npm toolchain (design §10).
#[derive(Debug, Clone)]
pub struct NpmToolchain {
    pub node_executable: PathBuf,
    pub npm_cli_js: PathBuf,
    pub npm_version: String,
    pub source: InstallationSource,
    /// The prefix used by `npm install --global` for this exact toolchain.
    pub global_prefix: PathBuf,
    /// The package root reported by `npm root --global`.
    pub global_root: PathBuf,
}

/// The detector contract (DI for tests).
#[async_trait::async_trait]
pub trait NpmToolchainDetector: Send + Sync {
    async fn detect(&self, node: &NodeInstallation)
        -> Result<NpmToolchain, PackageManagementError>;
}

/// Production detector: derives npm-cli.js candidates from the Node location,
/// canonicalizes, dedups and validates the first that runs.
pub struct DefaultNpmToolchainDetector {
    runner: Arc<dyn CommandRunner>,
}

impl DefaultNpmToolchainDetector {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        DefaultNpmToolchainDetector { runner }
    }
    pub fn with_default_runner() -> Self {
        Self::new(Arc::new(TokioCommandRunner))
    }
}

#[async_trait::async_trait]
impl NpmToolchainDetector for DefaultNpmToolchainDetector {
    async fn detect(
        &self,
        node: &NodeInstallation,
    ) -> Result<NpmToolchain, PackageManagementError> {
        for cand in candidate_npm_cli(node) {
            let Ok(canon) = std::fs::canonicalize(&cand) else {
                continue;
            };
            if !canon.is_file() {
                continue;
            }
            // Validate: run `<node> <npm-cli.js> --version`.
            let out = match self
                .runner
                .run(
                    &node.canonical_executable,
                    &[canon.to_string_lossy().as_ref(), "--version"],
                    None,
                    SHORT_TIMEOUT,
                    &[],
                )
                .await
            {
                Ok(o) => o,
                Err(_) => continue,
            };
            if !matches!(out.exit_code, Some(0)) {
                continue;
            }
            let version = match parse_npm_version(&out.stdout) {
                Some(v) => v,
                None => continue,
            };
            let Some(global_prefix) = self
                .query_global_path(node, &canon, &["prefix", "--global"])
                .await
            else {
                continue;
            };
            let Some(global_root) = self
                .query_global_path(node, &canon, &["root", "--global"])
                .await
            else {
                continue;
            };
            if !global_root.starts_with(&global_prefix) {
                continue;
            }
            // Containment: the npm-cli.js must live under a node_modules/npm
            // tree, and ideally not escape the node install area. We only
            // accept candidates derived from known layouts (below), so this is
            // already bounded.
            return Ok(NpmToolchain {
                node_executable: node.canonical_executable.clone(),
                npm_cli_js: canon,
                npm_version: version,
                source: node.source,
                global_prefix,
                global_root,
            });
        }
        Err(PackageManagementError::NpmUnavailable)
    }
}

impl DefaultNpmToolchainDetector {
    async fn query_global_path(
        &self,
        node: &NodeInstallation,
        npm_cli: &Path,
        npm_args: &[&str],
    ) -> Option<PathBuf> {
        let mut args = Vec::with_capacity(npm_args.len() + 1);
        args.push(npm_cli.to_string_lossy().to_string());
        args.extend(npm_args.iter().map(|arg| (*arg).to_string()));
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let out = self
            .runner
            .run(&node.canonical_executable, &refs, None, SHORT_TIMEOUT, &[])
            .await
            .ok()?;
        if !matches!(out.exit_code, Some(0)) {
            return None;
        }
        let raw = PathBuf::from(out.stdout.trim());
        if !raw.is_absolute() {
            return None;
        }
        // The root may not exist before the first global package is installed.
        // Canonicalize the nearest existing path while preserving the suffix.
        canonicalize_allow_missing(&raw)
    }
}

fn canonicalize_allow_missing(path: &Path) -> Option<PathBuf> {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return Some(canon);
    }
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        missing.push(cursor.file_name()?.to_os_string());
        cursor = cursor.parent()?;
    }
    let mut resolved = std::fs::canonicalize(cursor).ok()?;
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Some(resolved)
}

impl DefaultNpmToolchainDetector {
    #[allow(dead_code)]
    pub fn for_product(&self, _product: ProductId) -> &Self {
        // npm is product-agnostic; retained for API symmetry.
        self
    }
}

/// Build ordered npm-cli.js candidates from both the discovered Node path and
/// its canonical target. Keeping the discovered path is required for package
/// managers such as Homebrew, where `/opt/homebrew/bin/node` resolves into the
/// Cellar but global npm packages remain under `/opt/homebrew/lib`.
fn candidate_npm_cli(node: &NodeInstallation) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for node_path in [&node.executable, &node.canonical_executable] {
        if let Some(bin) = node_path.parent() {
            // Most Unix layouts: <prefix>/bin/node + <prefix>/lib/node_modules/npm
            out.push(bin.join("../lib/node_modules/npm/bin/npm-cli.js"));
            // Some layouts ship under <prefix>/node_modules.
            out.push(bin.join("../node_modules/npm/bin/npm-cli.js"));
            if let Some(root) = bin.parent() {
                out.push(root.join("lib/node_modules/npm/bin/npm-cli.js"));
                out.push(root.join("node_modules/npm/bin/npm-cli.js"));
            }
        }

        // A persisted Homebrew Node path may already be canonicalized to
        // `<prefix>/Cellar/node/<version>/bin/node`, losing the stable
        // `<prefix>/bin/node` discovery path. Recover only the prefix that
        // owns this exact Cellar tree; do not fall back to unrelated global
        // roots.
        if let Some(cellar) = node_path
            .ancestors()
            .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "Cellar"))
        {
            if let Some(prefix) = cellar.parent() {
                out.push(prefix.join("lib/node_modules/npm/bin/npm-cli.js"));
            }
        }
    }
    dedup_canon(out)
}

/// Canonicalize-then-dedupe so overlapping relative hints collapse.
fn dedup_canon(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if seen.insert(key.clone()) {
            out.push(p);
        }
    }
    out
}

/// Parse npm `--version` output (e.g. "10.8.2\n" or "npm/10.8.2").
fn parse_npm_version(stdout: &str) -> Option<String> {
    let trimmed = stdout.trim();
    // Take the first dotted-numeric token.
    let token = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let cleaned = token.rsplit_once('/').map(|(_, v)| v).unwrap_or(token);
    // Validate it looks like semver core.
    semver::Version::parse(cleaned.split('-').next()?).ok()?;
    Some(cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LocalRuntimeError;
    use crate::local_runtime::detector::CommandOutput;
    use async_trait::async_trait;
    use std::path::Path;

    /// A fake runner that returns a preset version for the npm probe and
    /// echoes node --version otherwise. Backed by a real tempdir so the
    /// canonicalize step has files to resolve.
    struct FakeRunner {
        npm_version: Option<String>,
        prefix: PathBuf,
    }
    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(
            &self,
            _program: &Path,
            args: &[&str],
            _cwd: Option<&Path>,
            _timeout: Duration,
            _env: &[(&str, &str)],
        ) -> Result<CommandOutput, LocalRuntimeError> {
            if args.contains(&"--version") {
                let stdout = self
                    .npm_version
                    .clone()
                    .unwrap_or_else(|| "10.8.2".to_string());
                return Ok(CommandOutput {
                    exit_code: Some(0),
                    stdout,
                    stderr: "".into(),
                });
            }
            if args.contains(&"prefix") {
                return Ok(CommandOutput {
                    exit_code: Some(0),
                    stdout: self.prefix.to_string_lossy().into_owned(),
                    stderr: "".into(),
                });
            }
            if args.contains(&"root") {
                return Ok(CommandOutput {
                    exit_code: Some(0),
                    stdout: self
                        .prefix
                        .join("lib/node_modules")
                        .to_string_lossy()
                        .into_owned(),
                    stderr: "".into(),
                });
            }
            Ok(CommandOutput {
                exit_code: Some(0),
                stdout: "ok".into(),
                stderr: "".into(),
            })
        }
    }

    fn make_layout(dir: &Path) -> NodeInstallation {
        // <dir>/bin/node + <dir>/lib/node_modules/npm/bin/npm-cli.js
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("lib/node_modules/npm/bin")).unwrap();
        let node = dir.join("bin/node");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        let npm = dir.join("lib/node_modules/npm/bin/npm-cli.js");
        std::fs::write(&npm, "// npm\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&node).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&node, perms).unwrap();
        }
        let canon = std::fs::canonicalize(&node).unwrap();
        NodeInstallation {
            executable: node,
            canonical_executable: canon,
            version: "24.19.0".into(),
            source: InstallationSource::Homebrew,
        }
    }

    #[tokio::test]
    async fn detects_paired_npm() {
        let dir = tempfile::tempdir().unwrap();
        let node = make_layout(dir.path());
        let det = DefaultNpmToolchainDetector::new(Arc::new(FakeRunner {
            npm_version: Some("10.8.2".into()),
            prefix: dir.path().to_path_buf(),
        }));
        let tc = det.detect(&node).await.unwrap();
        assert!(tc.npm_cli_js.ends_with("npm-cli.js"));
        assert_eq!(tc.npm_version, "10.8.2");
        assert_eq!(tc.node_executable, node.canonical_executable);
        assert_eq!(tc.global_prefix, std::fs::canonicalize(dir.path()).unwrap());
        assert!(tc.global_root.ends_with("lib/node_modules"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detects_homebrew_npm_from_noncanonical_node_prefix() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("homebrew");
        let cellar = prefix.join("Cellar/node/24.10.0");
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::create_dir_all(cellar.join("bin")).unwrap();
        std::fs::create_dir_all(prefix.join("lib/node_modules/npm/bin")).unwrap();
        let real_node = cellar.join("bin/node");
        std::fs::write(&real_node, "#!/bin/sh\n").unwrap();
        let discovered_node = prefix.join("bin/node");
        symlink(&real_node, &discovered_node).unwrap();
        std::fs::write(
            prefix.join("lib/node_modules/npm/bin/npm-cli.js"),
            "// npm\n",
        )
        .unwrap();

        let node = NodeInstallation {
            // Simulate the persisted setting seen in a signed/Finder-launched
            // app: only the canonical Cellar path remains available.
            executable: real_node.clone(),
            canonical_executable: std::fs::canonicalize(&real_node).unwrap(),
            version: "24.10.0".into(),
            source: InstallationSource::Homebrew,
        };
        let det = DefaultNpmToolchainDetector::new(Arc::new(FakeRunner {
            npm_version: Some("11.6.0".into()),
            prefix: prefix.clone(),
        }));

        let toolchain = det.detect(&node).await.unwrap();
        assert_eq!(toolchain.npm_version, "11.6.0");
        assert!(toolchain
            .npm_cli_js
            .ends_with("lib/node_modules/npm/bin/npm-cli.js"));
        assert_eq!(
            toolchain.global_prefix,
            std::fs::canonicalize(prefix).unwrap()
        );
        assert_ne!(node.executable, discovered_node);
    }

    #[tokio::test]
    async fn errors_when_no_npm_present() {
        let dir = tempfile::tempdir().unwrap();
        let node = make_layout(dir.path());
        // Remove the npm tree so no candidate resolves.
        std::fs::remove_dir_all(dir.path().join("lib")).unwrap();
        let det = DefaultNpmToolchainDetector::new(Arc::new(FakeRunner {
            npm_version: Some("10.8.2".into()),
            prefix: dir.path().to_path_buf(),
        }));
        assert!(matches!(
            det.detect(&node).await,
            Err(PackageManagementError::NpmUnavailable)
        ));
    }

    #[test]
    fn parses_npm_version_variants() {
        assert_eq!(parse_npm_version("10.8.2\n").as_deref(), Some("10.8.2"));
        assert_eq!(parse_npm_version("npm/10.8.2").as_deref(), Some("10.8.2"));
        assert!(parse_npm_version("garbage").is_none());
    }
}
