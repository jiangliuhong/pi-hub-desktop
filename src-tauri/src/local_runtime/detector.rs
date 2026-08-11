//! Installation discovery + validation (docs/design-v2.md §5, §6).
//!
//! Finds Node.js, Pi Hub and the optional external `pi` CLI without relying on
//! an interactive shell PATH (requirements-v2 §8.2, V2-FR-002). Candidates are
//! strictly validated — file name alone is never enough: package identity, bin
//! entry, version and (for Pi Hub) the production build must all check out
//! (design-v2 §6.4). No `sh -c` is ever used: every probe is an absolute
//! executable + fixed args (V2-SR-001).

use crate::error::LocalRuntimeError;
use crate::local_runtime::model::{
    InstallationSet, InstallationSource, NodeInstallation, PiCliInstallation, PiCliKind,
    PiHubInstallation, NODE_REQUIRED_MAJOR, NODE_REQUIRED_MINOR,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Hints handed to the detector: persisted user choices + the App PATH.
#[derive(Debug, Clone, Default)]
pub struct DetectionHints {
    /// Persisted absolute Node executable from settings (highest priority).
    pub persisted_node: Option<PathBuf>,
    /// Persisted absolute Pi Hub entrypoint from settings.
    pub persisted_pi_hub_entrypoint: Option<PathBuf>,
    /// Persisted Pi Hub package root (paired with the entrypoint).
    pub persisted_pi_hub_package_root: Option<PathBuf>,
    /// Extra directories to scan (typically the App process PATH entries).
    pub path_dirs: Vec<PathBuf>,
    /// Override for `$HOME`; production uses the real home. Tests point this
    /// at a tempdir so the version-manager layout is fully controlled.
    pub home_override: Option<PathBuf>,
}

/// Abstracted command execution so the detector is unit-testable without a
/// real Node/Pi Hub on the host. The production runner shells out via
/// `tokio::process` with a short timeout (design-v2 §6.4, NFR-001).
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(
        &self,
        program: &Path,
        args: &[&str],
        cwd: Option<&Path>,
        timeout: Duration,
        extra_env: &[(&str, &str)],
    ) -> Result<CommandOutput, LocalRuntimeError>;
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}

/// Production `CommandRunner`: executes an absolute program with fixed args and
/// a hard timeout. Never invokes a shell (V2-SR-001).
pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(
        &self,
        program: &Path,
        args: &[&str],
        cwd: Option<&Path>,
        timeout: Duration,
        extra_env: &[(&str, &str)],
    ) -> Result<CommandOutput, LocalRuntimeError> {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| LocalRuntimeError::NodeExecutionFailed(format!("spawn: {e}")))?;
        let limited = tokio::time::timeout(timeout, child.wait_with_output());
        match limited.await {
            Ok(Ok(out)) => Ok(CommandOutput {
                exit_code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            }),
            Ok(Err(e)) => Err(LocalRuntimeError::NodeExecutionFailed(format!("wait: {e}"))),
            Err(_) => Err(LocalRuntimeError::NodeExecutionFailed(
                "command timed out".into(),
            )),
        }
    }
}

/// The installation detector contract (design-v2 §6).
#[async_trait]
pub trait InstallationDetector: Send + Sync {
    async fn detect(&self, hints: &DetectionHints) -> Result<InstallationSet, LocalRuntimeError>;
}

/// Default detector. Holds no mutable state; cheap to clone/share.
pub struct DefaultInstallationDetector {
    runner: std::sync::Arc<dyn CommandRunner>,
}

impl DefaultInstallationDetector {
    pub fn new(runner: std::sync::Arc<dyn CommandRunner>) -> Self {
        DefaultInstallationDetector { runner }
    }

    pub fn with_default_runner() -> Self {
        Self::new(std::sync::Arc::new(TokioCommandRunner))
    }
}

#[async_trait]
impl InstallationDetector for DefaultInstallationDetector {
    async fn detect(&self, hints: &DetectionHints) -> Result<InstallationSet, LocalRuntimeError> {
        let home = home_dir(hints);

        // 1. Persisted Node (validate it actually works).
        let mut node: Option<NodeInstallation> = None;
        if let Some(p) = &hints.persisted_node {
            node = self
                .probe_node(p, InstallationSource::Persisted, &home)
                .await
                .ok()
                .flatten();
        }

        // 2. Scan candidate Node paths until a compatible one is found.
        if node.is_none() {
            for (cand, src) in candidate_node_paths(&home, &hints.path_dirs) {
                if let Some(n) = self.probe_node(&cand, src, &home).await.ok().flatten() {
                    node = Some(n);
                    break;
                }
            }
        }

        // 3. Persisted Pi Hub entrypoint (paired with the discovered/persisted node).
        let mut pi_hub: Option<PiHubInstallation> = None;
        if let (Some(entry), pkg_root) = (
            hints.persisted_pi_hub_entrypoint.as_deref(),
            hints.persisted_pi_hub_package_root.as_deref(),
        ) {
            if let Some(node) = &node {
                pi_hub = self
                    .probe_pi_hub(entry, pkg_root, node, InstallationSource::Persisted)
                    .await
                    .ok()
                    .flatten();
            }
        }
        if pi_hub.is_none() {
            if let Some(node) = &node {
                for (cand, src) in candidate_pi_hub_paths(&home, &hints.path_dirs) {
                    if let Some(p) = self
                        .probe_pi_hub(&cand, None, node, src)
                        .await
                        .ok()
                        .flatten()
                    {
                        pi_hub = Some(p);
                        break;
                    }
                }
            }
        }

        // 4. Optional external Pi CLI — informational only (design-v2 §6.6).
        let pi_cli = self.detect_pi_cli(&home, &hints.path_dirs).await;

        Ok(InstallationSet {
            node,
            pi_hub,
            pi_cli,
        })
    }
}

impl DefaultInstallationDetector {
    async fn probe_node(
        &self,
        path: &Path,
        source: InstallationSource,
        _home: &Path,
    ) -> Result<Option<NodeInstallation>, LocalRuntimeError> {
        let canon = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        if !is_executable(&canon) {
            return Ok(None);
        }
        let out = self
            .runner
            .run(&canon, &["--version"], None, SHORT_TIMEOUT, &[])
            .await?;
        if !is_success(&out) {
            return Ok(None);
        }
        let version = match parse_node_version(&out.stdout) {
            Some(v) => v,
            None => return Ok(None),
        };
        if !node_satisfies_baseline(&version) {
            // Still return it so the caller can report incompatible; but for
            // auto-selection we skip incompatible nodes (design-v2 §6.5).
            return Ok(None);
        }
        Ok(Some(NodeInstallation {
            executable: path.to_path_buf(),
            canonical_executable: canon,
            version: version.to_string(),
            source,
        }))
    }

    async fn probe_pi_hub(
        &self,
        bin_entry: &Path,
        pkg_root_hint: Option<&Path>,
        node: &NodeInstallation,
        source: InstallationSource,
    ) -> Result<Option<PiHubInstallation>, LocalRuntimeError> {
        let canon = match std::fs::canonicalize(bin_entry) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let facts = match locate_and_validate_package(&canon, pkg_root_hint) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };
        // Verify the live entry reports the right identity via the candidate node.
        let out = self
            .runner
            .run(
                &node.canonical_executable,
                &[canon.to_string_lossy().as_ref(), "--version", "--json"],
                Some(&facts.package_root),
                SHORT_TIMEOUT,
                &[],
            )
            .await?;
        if !is_success(&out) {
            return Ok(None);
        }
        let live = match serde_json::from_str::<VersionJson>(&out.stdout) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        if live.name != FACTS_EXPECTED_PACKAGE {
            return Ok(None);
        }
        Ok(Some(PiHubInstallation {
            package_root: facts.package_root,
            entrypoint: canon,
            version: live.version,
            node_requirement: live.node_requirement.unwrap_or_default(),
            source,
        }))
    }

    async fn detect_pi_cli(&self, home: &Path, path_dirs: &[PathBuf]) -> Option<PiCliInstallation> {
        for (cand, src) in candidate_pi_cli_paths(home, path_dirs) {
            let canon = std::fs::canonicalize(&cand).ok()?;
            if !is_executable(&canon) {
                continue;
            }
            let out = self
                .runner
                .run(&canon, &["--version"], None, SHORT_TIMEOUT, &[])
                .await
                .ok()?;
            if !is_success(&out) {
                continue;
            }
            let version = out.stdout.trim().strip_prefix("pi ").map(|s| s.to_string());
            return Some(PiCliInstallation {
                executable: canon,
                version,
                kind: PiCliKind::Unknown,
                source: src,
            });
        }
        None
    }
}

const SHORT_TIMEOUT: Duration = Duration::from_secs(8);
const FACTS_EXPECTED_PACKAGE: &str = "@jarome/pi-hub";

#[derive(Debug, Deserialize)]
struct VersionJson {
    name: String,
    version: String,
    #[serde(default)]
    node_requirement: Option<String>,
}

/// Facts derived from `package.json` adjacent to a bin entry.
struct PiHubPackageFacts {
    package_root: PathBuf,
}

/// Locate the `package.json` for a candidate bin entry and validate identity
/// (design-v2 §6.4). Walks up from the entry until a `package.json` declaring
/// `@jarome/pi-hub` with a matching `bin.pi-hub` is found, bounded so we never
/// walk to the filesystem root.
fn locate_and_validate_package(
    entry: &Path,
    pkg_root_hint: Option<&Path>,
) -> Result<PiHubPackageFacts, LocalRuntimeError> {
    if let Some(hint) = pkg_root_hint {
        if let Ok(facts) = validate_root(hint, entry) {
            return Ok(facts);
        }
    }
    // Walk up from the entry's directory.
    let mut dir = entry
        .parent()
        .ok_or_else(|| LocalRuntimeError::PiHubInstallationInvalid("no parent".into()))?;
    for _ in 0..8 {
        if let Ok(facts) = validate_root(dir, entry) {
            return Ok(facts);
        }
        dir = match dir.parent() {
            Some(p) => p,
            None => break,
        };
    }
    Err(LocalRuntimeError::PiHubInstallationInvalid(
        "no matching package.json".into(),
    ))
}

fn validate_root(root: &Path, entry: &Path) -> Result<PiHubPackageFacts, LocalRuntimeError> {
    let pkg_path = root.join("package.json");
    let raw = std::fs::read_to_string(&pkg_path)
        .map_err(|_| LocalRuntimeError::PiHubInstallationInvalid("no package.json".into()))?;
    let pkg: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| LocalRuntimeError::PiHubInstallationInvalid(format!("pkg json: {e}")))?;
    let name = pkg
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LocalRuntimeError::PiHubInstallationInvalid("no name".into()))?;
    if name != FACTS_EXPECTED_PACKAGE {
        return Err(LocalRuntimeError::PiHubInstallationInvalid(
            "name mismatch".into(),
        ));
    }
    // bin.pi-hub should resolve (after canonicalize) to the candidate entry.
    if let Some(bin) = pkg.get("bin") {
        let bin_target: Option<PathBuf> = if let Some(s) = bin.as_str() {
            Some(root.join(s))
        } else if let Some(m) = bin.as_object() {
            m.get("pi-hub")
                .and_then(|v| v.as_str())
                .map(|s| root.join(s))
        } else {
            None
        };
        if let Some(target) = bin_target {
            if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&target), entry.canonicalize()) {
                if a == b {
                    return Ok(PiHubPackageFacts {
                        package_root: root.to_path_buf(),
                    });
                }
            }
        }
    }
    // Production build marker: `.next` directory exists in package root.
    if root.join(".next").is_dir() {
        return Ok(PiHubPackageFacts {
            package_root: root.to_path_buf(),
        });
    }
    Err(LocalRuntimeError::PiHubInstallationInvalid(
        "bin/.next mismatch".into(),
    ))
}

/// Parse `v24.19.0`-style output into a semver version.
pub(crate) fn parse_node_version(stdout: &str) -> Option<semver::Version> {
    let trimmed = stdout.trim();
    let stripped = trimmed.strip_prefix('v').unwrap_or(trimmed);
    // Node pre-release labels exist but we only compare the numeric core.
    semver::Version::parse(stripped.split('-').next()?).ok()
}

/// Whether a Node version satisfies the Pi Hub baseline (requirements-v2 §8.2).
pub(crate) fn node_satisfies_baseline(v: &semver::Version) -> bool {
    if v.major != NODE_REQUIRED_MAJOR {
        return v.major > NODE_REQUIRED_MAJOR;
    }
    if v.minor != NODE_REQUIRED_MINOR {
        return v.minor > NODE_REQUIRED_MINOR;
    }
    // major and minor match the baseline; the baseline patch is 0 so any
    // patch satisfies it.
    true
}

fn is_success(out: &CommandOutput) -> bool {
    matches!(out.exit_code, Some(0))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn home_dir(hints: &DetectionHints) -> PathBuf {
    hints
        .home_override
        .clone()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

// ---- candidate path enumeration (design-v2 §6.2 / §6.3) ----

/// Ordered Node candidate paths. Order encodes discovery priority (design-v2
/// §6.1): PATH dirs first, then fixed roots, then version managers.
pub fn candidate_node_paths(
    home: &Path,
    path_dirs: &[PathBuf],
) -> Vec<(PathBuf, InstallationSource)> {
    let mut out: Vec<(PathBuf, InstallationSource)> = Vec::new();
    // Current App PATH (resolved `node`).
    for dir in path_dirs {
        out.push((dir.join("node"), InstallationSource::Path));
    }
    // Fixed system roots.
    out.push((
        "/opt/homebrew/bin/node".into(),
        InstallationSource::Homebrew,
    ));
    out.push(("/usr/local/bin/node".into(), InstallationSource::Homebrew));
    out.push(("/usr/bin/node".into(), InstallationSource::Path));
    // Version managers.
    out.extend(version_manager_bin(
        home,
        "volta",
        "node",
        InstallationSource::Volta,
    ));
    out.extend(glob_children(
        &home.join(".nvm/versions/node"),
        "bin/node",
        InstallationSource::Nvm,
    ));
    out.extend(glob_children(
        &home.join(".local/share/fnm/node-versions"),
        "installation/bin/node",
        InstallationSource::Fnm,
    ));
    out.push((home.join(".asdf/shims/node"), InstallationSource::Asdf));
    out.push((
        home.join(".local/share/mise/shims/node"),
        InstallationSource::Mise,
    ));
    dedup_keep_order(out)
}

pub fn candidate_pi_hub_paths(
    home: &Path,
    path_dirs: &[PathBuf],
) -> Vec<(PathBuf, InstallationSource)> {
    let mut out: Vec<(PathBuf, InstallationSource)> = Vec::new();
    for dir in path_dirs {
        out.push((dir.join("pi-hub"), InstallationSource::Path));
    }
    out.push((
        "/opt/homebrew/bin/pi-hub".into(),
        InstallationSource::Homebrew,
    ));
    out.push(("/usr/local/bin/pi-hub".into(), InstallationSource::Homebrew));
    out.extend(version_manager_bin(
        home,
        "volta",
        "pi-hub",
        InstallationSource::Volta,
    ));
    out.extend(glob_children(
        &home.join(".nvm/versions/node"),
        "bin/pi-hub",
        InstallationSource::Nvm,
    ));
    out.extend(glob_children(
        &home.join(".local/share/fnm/node-versions"),
        "installation/bin/pi-hub",
        InstallationSource::Fnm,
    ));
    out.push((home.join(".asdf/shims/pi-hub"), InstallationSource::Asdf));
    out.push((
        home.join(".local/share/mise/shims/pi-hub"),
        InstallationSource::Mise,
    ));
    dedup_keep_order(out)
}

pub fn candidate_pi_cli_paths(
    home: &Path,
    path_dirs: &[PathBuf],
) -> Vec<(PathBuf, InstallationSource)> {
    let mut out: Vec<(PathBuf, InstallationSource)> = Vec::new();
    for dir in path_dirs {
        out.push((dir.join("pi"), InstallationSource::Path));
    }
    out.push(("/opt/homebrew/bin/pi".into(), InstallationSource::Homebrew));
    out.push(("/usr/local/bin/pi".into(), InstallationSource::Homebrew));
    out.extend(version_manager_bin(
        home,
        "volta",
        "pi",
        InstallationSource::Volta,
    ));
    out.extend(glob_children(
        &home.join(".nvm/versions/node"),
        "bin/pi",
        InstallationSource::Nvm,
    ));
    out.push((home.join(".asdf/shims/pi"), InstallationSource::Asdf));
    out.push((
        home.join(".local/share/mise/shims/pi"),
        InstallationSource::Mise,
    ));
    dedup_keep_order(out)
}

/// `<home>/.<vm>/bin/<name>` (Volta layout).
fn version_manager_bin(
    home: &Path,
    vm: &str,
    name: &str,
    src: InstallationSource,
) -> Vec<(PathBuf, InstallationSource)> {
    vec![(home.join(format!(".{vm}/bin")).join(name), src)]
}

/// Expand `<root>/*/<suffix>` style version-manager layouts (NVM/FNM) by
/// enumerating child directories of `root`. Returns candidate paths in
/// directory-name descending order (newest first), matching §6.5.
fn glob_children(
    root: &Path,
    suffix: &str,
    src: InstallationSource,
) -> Vec<(PathBuf, InstallationSource)> {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.is_dir() {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    dirs.sort_by(|a, b| b.cmp(a));
    dirs.into_iter().map(|d| (d.join(suffix), src)).collect()
}

fn dedup_keep_order(v: Vec<(PathBuf, InstallationSource)>) -> Vec<(PathBuf, InstallationSource)> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter()
        .filter(|(p, _)| seen.insert(p.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::fs;

    #[test]
    fn parse_node_version_handles_v_prefix() {
        let v = parse_node_version("v24.19.0\n").unwrap();
        assert_eq!(v.major, 24);
        assert_eq!(v.minor, 19);
    }

    #[test]
    fn baseline_check_major_bump_passes() {
        assert!(node_satisfies_baseline(&semver::Version::new(25, 0, 0)));
        assert!(!node_satisfies_baseline(&semver::Version::new(21, 0, 0)));
    }

    #[test]
    fn baseline_check_minor_and_patch() {
        assert!(node_satisfies_baseline(&semver::Version::new(22, 19, 0)));
        assert!(node_satisfies_baseline(&semver::Version::new(22, 20, 0)));
        assert!(!node_satisfies_baseline(&semver::Version::new(22, 18, 99)));
    }

    #[test]
    fn candidate_node_paths_includes_version_managers() {
        let home = PathBuf::from("/tmp/fake-home");
        let cands = candidate_node_paths(&home, &[]);
        assert!(cands
            .iter()
            .any(|(p, _)| p == &PathBuf::from("/tmp/fake-home/.volta/bin/node")));
        assert!(cands
            .iter()
            .any(|(p, _)| p == &PathBuf::from("/tmp/fake-home/.asdf/shims/node")));
        assert!(cands
            .iter()
            .any(|(p, _)| p == &PathBuf::from("/opt/homebrew/bin/node")));
    }

    #[test]
    fn candidate_paths_are_deduped() {
        let home = PathBuf::from("/h");
        let path_dirs = vec![PathBuf::from("/opt/homebrew/bin")];
        let cands = candidate_node_paths(&home, &path_dirs);
        let count = cands
            .iter()
            .filter(|(p, _)| p == &PathBuf::from("/opt/homebrew/bin/node"))
            .count();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn glob_children_enumerates_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".nvm/versions/node");
        fs::create_dir_all(root.join("v24.19.0/bin")).await.unwrap();
        fs::create_dir_all(root.join("v22.5.0/bin")).await.unwrap();
        fs::create_dir_all(root.join("v23.0.0/bin")).await.unwrap();
        let home = dir.path().to_path_buf();
        let cands = glob_children(
            &home.join(".nvm/versions/node"),
            "bin/node",
            InstallationSource::Nvm,
        );
        // Descending order.
        assert_eq!(
            cands[0].0,
            home.join(".nvm/versions/node/v24.19.0/bin/node")
        );
        assert_eq!(cands[1].0, home.join(".nvm/versions/node/v23.0.0/bin/node"));
        assert_eq!(cands[2].0, home.join(".nvm/versions/node/v22.5.0/bin/node"));
    }

    /// A fake runner keyed by path. It checks arguments first (so `node
    /// <entry> --version --json` resolves to the entry's canned output), then
    /// falls back to the program path (`node --version`).
    struct FakeRunner {
        responses: Mutex<HashMap<PathBuf, CommandOutput>>,
    }
    impl FakeRunner {
        fn new() -> Self {
            FakeRunner {
                responses: Mutex::new(HashMap::new()),
            }
        }
        fn set(&self, program: &Path, out: CommandOutput) {
            self.responses
                .lock()
                .unwrap()
                .insert(program.to_path_buf(), out);
        }
    }
    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(
            &self,
            program: &Path,
            args: &[&str],
            _cwd: Option<&Path>,
            _timeout: Duration,
            _extra_env: &[(&str, &str)],
        ) -> Result<CommandOutput, LocalRuntimeError> {
            let map = self.responses.lock().unwrap();
            // An arg that is a registered path wins (handles `node <entry>`).
            for a in args {
                if let Some(out) = map.get(Path::new(a)) {
                    return Ok(out.clone());
                }
            }
            map.get(program)
                .cloned()
                .ok_or_else(|| LocalRuntimeError::NodeExecutionFailed("no fake".into()))
        }
    }

    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[tokio::test]
    async fn detect_finds_compatible_node() {
        let dir = tempfile::tempdir().unwrap();
        let node_path = dir.path().join("bin/node");
        make_executable(&node_path);
        let runner = std::sync::Arc::new(FakeRunner::new());
        runner.set(
            &node_path,
            CommandOutput {
                exit_code: Some(0),
                stdout: "v24.19.0\n".into(),
                stderr: "".into(),
            },
        );
        let det = DefaultInstallationDetector::new(runner.clone());
        let hints = DetectionHints {
            path_dirs: vec![dir.path().join("bin")],
            home_override: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let set = det.detect(&hints).await.unwrap();
        let node = set.node.expect("node found");
        assert!(node.version.starts_with("24.19"));
    }

    #[tokio::test]
    async fn detect_skips_incompatible_node() {
        let dir = tempfile::tempdir().unwrap();
        let node_path = dir.path().join("bin/node");
        make_executable(&node_path);
        let runner = std::sync::Arc::new(FakeRunner::new());
        runner.set(
            &node_path,
            CommandOutput {
                exit_code: Some(0),
                stdout: "v18.0.0\n".into(),
                stderr: "".into(),
            },
        );
        let det = DefaultInstallationDetector::new(runner);
        let hints = DetectionHints {
            path_dirs: vec![dir.path().join("bin")],
            home_override: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let set = det.detect(&hints).await.unwrap();
        assert!(set.node.is_none(), "incompatible node must be skipped");
    }

    #[tokio::test]
    async fn detect_validates_pi_hub_package_identity() {
        let dir = tempfile::tempdir().unwrap();
        // The candidate path for `path_dirs=[bin]` is `bin/pi-hub`; lay the
        // entry there and the package.json at the dir root so walking up from
        // the entry resolves the package identity.
        let entry = dir.path().join("bin/pi-hub");
        make_executable(&entry);
        // package.json declares the right name + bin.
        fs::write(
            dir.path().join("package.json"),
            serde_json::json!({
                "name": "@jarome/pi-hub",
                "version": "0.0.42",
                "bin": { "pi-hub": "bin/pi-hub" },
                "engines": { "node": ">=22.19.0" }
            })
            .to_string(),
        )
        .await
        .unwrap();
        fs::create_dir_all(dir.path().join(".next")).await.unwrap();
        let node_path = dir.path().join("bin/node");
        make_executable(&node_path);
        let runner = std::sync::Arc::new(FakeRunner::new());
        runner.set(
            &node_path,
            CommandOutput {
                exit_code: Some(0),
                stdout: "v24.19.0\n".into(),
                stderr: "".into(),
            },
        );
        let entry_canon = std::fs::canonicalize(&entry).unwrap();
        runner.set(
            &entry_canon,
            CommandOutput {
                exit_code: Some(0),
                stdout: serde_json::json!({
                    "name": "@jarome/pi-hub",
                    "version": "0.0.42",
                    "nodeRequirement": ">=22.19.0"
                })
                .to_string(),
                stderr: "".into(),
            },
        );
        let det = DefaultInstallationDetector::new(runner);
        let hints = DetectionHints {
            path_dirs: vec![dir.path().join("bin")],
            home_override: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let set = det.detect(&hints).await.unwrap();
        let pi_hub = set.pi_hub.expect("pi hub found");
        assert_eq!(pi_hub.version, "0.0.42");
        assert_eq!(
            pi_hub.package_root,
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[tokio::test]
    async fn detect_rejects_wrong_package_name() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("bin/pi-hub");
        make_executable(&entry);
        fs::write(
            dir.path().join("package.json"),
            serde_json::json!({
                "name": "@evil/impostor",
                "bin": { "pi-hub": "bin/pi-hub" }
            })
            .to_string(),
        )
        .await
        .unwrap();
        fs::create_dir_all(dir.path().join(".next")).await.unwrap();
        let node_path = dir.path().join("bin/node");
        make_executable(&node_path);
        let runner = std::sync::Arc::new(FakeRunner::new());
        runner.set(
            &node_path,
            CommandOutput {
                exit_code: Some(0),
                stdout: "v24.19.0\n".into(),
                stderr: "".into(),
            },
        );
        let det = DefaultInstallationDetector::new(runner);
        let hints = DetectionHints {
            path_dirs: vec![dir.path().join("bin")],
            home_override: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let set = det.detect(&hints).await.unwrap();
        assert!(set.pi_hub.is_none(), "impostor package must be rejected");
    }
}
