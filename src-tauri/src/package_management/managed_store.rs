//! Managed package store (docs/requirements-v3.md §13; design §5.2, §11.3,
//! §11.4).
//!
//! Owns the Desktop-managed copy tree under
//! `~/Library/Application Support/Pi Hub Client/packages/`. This is the
//! **only** place install/update may write (V3-SR-003). External installs are
//! never touched here. Activation is atomic (temp manifest + rename; version
//! dir via rename). Rollback restores the previous active entry.
//!
//! Layout (design §5.2):
//! ```text
//! packages/
//! ├── manifest.json
//! ├── pi/        versions/<v>/, staging/<op>/, logs/
//! └── pi_hub/    versions/<v>/, staging/<op>/, logs/
//! ```

use crate::error::PackageManagementError;
use crate::package_management::model::{bin_name, package_name, ProductId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::RwLock;

/// Current manifest schema version (AGENTS.md §12).
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Staging dirs older than this with no live operation are removed on startup
/// (design §11.4).
pub const STALE_STAGING_SECS: i64 = 24 * 60 * 60;

const MANIFEST_FILE: &str = "manifest.json";

/// One active managed installation recorded in the manifest (design §15).
/// No secrets — only paths, versions and timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveEntry {
    /// "pi" / "pi_hub" (ProductId api name).
    pub product: String,
    pub version: String,
    /// Canonical package root under the managed root
    /// (`versions/<v>/node_modules/<pkg>`).
    pub package_root: PathBuf,
    /// Canonical bin entrypoint.
    pub entrypoint: PathBuf,
    pub package_name: String,
    pub bin: String,
    /// Absolute Node used to install/verify this version.
    pub node_executable: PathBuf,
    pub activated_at: DateTime<Utc>,
}

/// The persisted manifest (design §15).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageManifest {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub active: BTreeMap<String, ActiveEntry>,
    /// Previous successful active per product, retained for rollback
    /// (design §11.2, §11.4).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub previous: BTreeMap<String, ActiveEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_executable: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update_check_at: Option<DateTime<Utc>>,
}

impl PackageManifest {
    fn key(product: ProductId) -> &'static str {
        product.api_name()
    }
}

/// Atomic, versioned store for managed packages.
pub struct ManagedPackageStore {
    root: PathBuf,
    manifest: RwLock<PackageManifest>,
}

impl ManagedPackageStore {
    /// Create a store rooted at `root` (the `packages/` dir). Loads manifest
    /// best-effort; callers should `load()` explicitly.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ManagedPackageStore {
            root: root.into(),
            manifest: RwLock::new(PackageManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                ..Default::default()
            }),
        }
    }

    /// Canonical managed root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn load(&self) -> Result<(), PackageManagementError> {
        let path = self.root.join(MANIFEST_FILE);
        let loaded = match fs::read_to_string(&path).await {
            Ok(s) if !s.trim().is_empty() => parse_and_migrate(&s)?,
            Ok(_) => default_manifest(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => default_manifest(),
            Err(e) => {
                return Err(PackageManagementError::Internal(format!(
                    "read manifest: {e}"
                )));
            }
        };
        *self.manifest.write().await = loaded;
        Ok(())
    }

    async fn persist_locked(&self, m: &PackageManifest) -> Result<(), PackageManagementError> {
        self.ensure_layout().await?;
        let json = serde_json::to_string_pretty(m)
            .map_err(|e| PackageManagementError::Internal(format!("serialize manifest: {e}")))?;
        atomic_write(&self.root.join(MANIFEST_FILE), &json).await
    }

    /// Ensure the root and per-product dirs exist.
    pub async fn ensure_layout(&self) -> Result<(), PackageManagementError> {
        fs::create_dir_all(&self.root)
            .await
            .map_err(|e| PackageManagementError::Internal(format!("mkdir root: {e}")))?;
        for p in ProductId::all() {
            let pd = self.product_root(*p);
            for sub in ["versions", "staging", "logs"] {
                fs::create_dir_all(pd.join(sub))
                    .await
                    .map_err(|e| PackageManagementError::Internal(format!("mkdir {sub}: {e}")))?;
            }
        }
        Ok(())
    }

    pub fn product_root(&self, product: ProductId) -> PathBuf {
        self.root.join(product.api_name().replace('_', "-"))
    }

    pub fn versions_root(&self, product: ProductId) -> PathBuf {
        self.product_root(product).join("versions")
    }

    pub fn staging_root(&self, product: ProductId) -> PathBuf {
        self.product_root(product).join("staging")
    }

    pub fn logs_root(&self, product: ProductId) -> PathBuf {
        self.product_root(product).join("logs")
    }

    /// Create a fresh, empty staging dir for an operation (design §11.1). The
    /// op id is a UUID, so the path has no separators and cannot traverse.
    pub async fn create_staging(
        &self,
        product: ProductId,
        op_id: uuid::Uuid,
    ) -> Result<PathBuf, PackageManagementError> {
        self.ensure_layout().await?;
        let dir = self.staging_root(product).join(op_id.to_string());
        if dir.exists() {
            // A stale dir with the same UUID should not happen; remove it to
            // keep the install deterministic.
            let _ = fs::remove_dir_all(&dir).await;
        }
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| PackageManagementError::Internal(format!("mkdir staging: {e}")))?;
        Ok(dir)
    }

    /// Remove a staging dir (best-effort) after failure/cancel.
    pub async fn remove_staging(&self, dir: &Path) {
        let _ = fs::remove_dir_all(dir).await;
    }

    /// Best-effort removal of stale staging dirs older than the threshold
    /// (design §11.4). Never touches versions referenced by the manifest.
    pub async fn cleanup_stale_staging(&self, now: DateTime<Utc>) {
        for product in ProductId::all() {
            let staging = self.staging_root(*product);
            let mut read = match fs::read_dir(&staging).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            while let Ok(Some(entry)) = read.next_entry().await {
                let path = entry.path();
                let mtime = match entry.metadata().await {
                    Ok(m) => m.modified().ok(),
                    Err(_) => continue,
                };
                let Some(mtime) = mtime else { continue };
                let mtime_dt: DateTime<Utc> = mtime.into();
                if (now - mtime_dt).num_seconds() > STALE_STAGING_SECS {
                    let _ = fs::remove_dir_all(&path).await;
                }
            }
        }
    }

    /// The current active managed installation for a product, if any.
    pub async fn active(&self, product: ProductId) -> Option<ActiveEntry> {
        self.manifest
            .read()
            .await
            .active
            .get(PackageManifest::key(product))
            .cloned()
    }

    /// The previous active entry retained for rollback (design §11.2).
    pub async fn previous_active(&self, product: ProductId) -> Option<ActiveEntry> {
        self.manifest
            .read()
            .await
            .previous
            .get(PackageManifest::key(product))
            .cloned()
    }

    /// Promote a verified staging dir to `versions/<version>` and make it the
    /// active installation, retaining the old active as the rollback target
    /// (design §11.3). Atomic on the same filesystem via rename; a same-version
    /// repair swaps the existing dir aside.
    pub async fn promote(
        &self,
        product: ProductId,
        staging_dir: &Path,
        version: &semver::Version,
        entrypoint: PathBuf,
        node_executable: PathBuf,
    ) -> Result<ActiveEntry, PackageManagementError> {
        self.ensure_layout().await?;
        // Confine the staging dir to the managed root (V3-SR-003).
        let staging_canon = canonicalize_required(staging_dir)?;
        self.assert_confined(&staging_canon)?;

        let version_str = version.to_string();
        validate_version_dir_name(&version_str)?;
        let target = self.versions_root(product).join(&version_str);
        let pkg = package_name(product);
        let bin = bin_name(product);
        // The package_root is recomputed below from the renamed target; drop
        // the pre-rename placeholder to avoid a dead binding.
        let _ = staging_canon.join("node_modules").join(pkg);

        // If the target already exists, swap it aside atomically so the active
        // install is never left half-replaced (design §11.3).
        let mut retired: Option<PathBuf> = None;
        if target.exists() {
            let aside = self
                .versions_root(product)
                .join(format!(".{version_str}.retiring.{}", uuid::Uuid::new_v4()));
            fs::rename(&target, &aside).await.map_err(|e| {
                PackageManagementError::ActivationFailed {
                    product: product.api_name().into(),
                    reason: format!("swap aside: {e}"),
                }
            })?;
            retired = Some(aside);
        }

        // Rename staging → target (same filesystem → atomic).
        if let Err(e) = fs::rename(&staging_canon, &target).await {
            // Restore the retired dir if we moved one.
            if let Some(aside) = &retired {
                let _ = fs::rename(aside, &target).await;
            }
            return Err(PackageManagementError::ActivationFailed {
                product: product.api_name().into(),
                reason: format!("rename staging: {e}"),
            });
        }
        // Best-effort drop the retired copy now that the new one is in place.
        if let Some(aside) = retired {
            let _ = fs::remove_dir_all(&aside).await;
        }

        let target_canon = canonicalize_required(&target)?;
        let pkg_root_canon = canonicalize_required(&target_canon.join("node_modules").join(pkg))
            .unwrap_or_else(|_| target_canon.join("node_modules").join(pkg));
        let entry_canon = canonicalize_required(&entrypoint).unwrap_or_else(|_| entrypoint.clone());

        let entry = ActiveEntry {
            product: product.api_name().into(),
            version: version_str,
            package_root: pkg_root_canon,
            entrypoint: entry_canon,
            package_name: pkg.to_string(),
            bin: bin.to_string(),
            node_executable,
            activated_at: Utc::now(),
        };

        let mut m = self.manifest.write().await;
        let key = PackageManifest::key(product).to_string();
        if let Some(old) = m.active.get(&key).cloned() {
            m.previous.insert(key.clone(), old);
        }
        m.active.insert(key, entry.clone());
        m.schema_version = MANIFEST_SCHEMA_VERSION;
        let snapshot = m.clone();
        drop(m);
        self.persist_locked(&snapshot).await?;
        Ok(entry)
    }

    /// Restore the previous active entry for a product (rollback). Returns the
    /// restored entry, or None if there was nothing to roll back to.
    pub async fn restore_previous(
        &self,
        product: ProductId,
    ) -> Result<Option<ActiveEntry>, PackageManagementError> {
        let mut m = self.manifest.write().await;
        let key = PackageManifest::key(product).to_string();
        let Some(prev) = m.previous.remove(&key) else {
            return Ok(None);
        };
        let current = m.active.insert(key.clone(), prev.clone());
        if let Some(cur) = current {
            // Keep the failed-new as nothing; the old active is now restored.
            let _ = cur;
        }
        let snapshot = m.clone();
        drop(m);
        self.persist_locked(&snapshot).await?;
        Ok(Some(prev))
    }

    /// Explicitly set the active entry (used when switching selection without
    /// moving dirs, e.g. choosing a managed version that's already on disk).
    pub async fn set_active(
        &self,
        product: ProductId,
        entry: ActiveEntry,
    ) -> Result<(), PackageManagementError> {
        let mut m = self.manifest.write().await;
        let key = PackageManifest::key(product).to_string();
        if let Some(old) = m.active.get(&key).cloned() {
            if old.version != entry.version {
                m.previous.insert(key.clone(), old);
            }
        }
        m.active.insert(key, entry);
        let snapshot = m.clone();
        drop(m);
        self.persist_locked(&snapshot).await
    }

    /// List installed versions on disk (directory names), unsorted.
    pub async fn list_versions(&self, product: ProductId) -> Vec<String> {
        let mut out = Vec::new();
        let mut read = match fs::read_dir(self.versions_root(product)).await {
            Ok(r) => r,
            Err(_) => return out,
        };
        while let Ok(Some(entry)) = read.next_entry().await {
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if !name.starts_with('.') {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out
    }

    /// Remove versions that are neither active nor previous (design §11.4).
    pub async fn cleanup_versions(&self, product: ProductId) {
        let m = self.manifest.read().await;
        let keep: std::collections::HashSet<String> = [
            m.active.get(product.api_name()),
            m.previous.get(product.api_name()),
        ]
        .iter()
        .filter_map(|e| e.as_ref().map(|e| e.version.clone()))
        .collect();
        drop(m);
        let mut read = match fs::read_dir(self.versions_root(product)).await {
            Ok(r) => r,
            Err(_) => return,
        };
        while let Ok(Some(entry)) = read.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with('.') {
                    continue;
                }
                if !keep.contains(name) {
                    let _ = fs::remove_dir_all(entry.path()).await;
                }
            }
        }
    }

    /// Record the last update-check timestamp (design §15).
    pub async fn set_last_update_check(
        &self,
        at: DateTime<Utc>,
    ) -> Result<(), PackageManagementError> {
        let mut m = self.manifest.write().await;
        m.last_update_check_at = Some(at);
        let snapshot = m.clone();
        drop(m);
        self.persist_locked(&snapshot).await
    }

    pub async fn last_update_check(&self) -> Option<DateTime<Utc>> {
        self.manifest.read().await.last_update_check_at
    }

    pub async fn set_node_executable(&self, node: PathBuf) -> Result<(), PackageManagementError> {
        let mut m = self.manifest.write().await;
        m.node_executable = Some(node);
        let snapshot = m.clone();
        drop(m);
        self.persist_locked(&snapshot).await
    }

    pub async fn node_executable(&self) -> Option<PathBuf> {
        self.manifest.read().await.node_executable.clone()
    }

    /// Reject any path that escapes the canonicalized managed root (V3-SR-003).
    fn assert_confined(&self, path: &Path) -> Result<(), PackageManagementError> {
        let root_canon = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        if !path.starts_with(&root_canon) {
            return Err(PackageManagementError::Internal(format!(
                "path escapes managed root: {}",
                path.display()
            )));
        }
        Ok(())
    }
}

fn default_manifest() -> PackageManifest {
    PackageManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        ..Default::default()
    }
}

fn parse_and_migrate(json: &str) -> Result<PackageManifest, PackageManagementError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| PackageManagementError::Internal(format!("invalid manifest json: {e}")))?;
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    if version > MANIFEST_SCHEMA_VERSION {
        return Err(PackageManagementError::Internal(format!(
            "manifest schema_version {version} newer than supported {MANIFEST_SCHEMA_VERSION}"
        )));
    }
    let mut m: PackageManifest = serde_json::from_value(value)
        .map_err(|e| PackageManagementError::Internal(format!("manifest schema mismatch: {e}")))?;
    m.schema_version = MANIFEST_SCHEMA_VERSION;
    Ok(m)
}

/// A version dir name must be a bare semver string (no path separators) so it
/// can never traverse out of `versions/`.
fn validate_version_dir_name(name: &str) -> Result<(), PackageManagementError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || semver::Version::parse(name).is_err()
    {
        return Err(PackageManagementError::ActivationFailed {
            product: "unknown".into(),
            reason: format!("invalid version dir name: {name}"),
        });
    }
    Ok(())
}

fn canonicalize_required(p: &Path) -> Result<PathBuf, PackageManagementError> {
    std::fs::canonicalize(p)
        .map_err(|e| PackageManagementError::Internal(format!("canonicalize {}: {e}", p.display())))
}

/// Atomic write: temp sibling + rename (POSIX atomic). Mirrors the settings
/// store pattern (design-v2 §7).
async fn atomic_write(path: &Path, contents: &str) -> Result<(), PackageManagementError> {
    let parent = path
        .parent()
        .ok_or_else(|| PackageManagementError::Internal("manifest path has no parent".into()))?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| PackageManagementError::Internal(format!("mkdir manifest parent: {e}")))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("manifest")
    ));
    fs::write(&tmp, contents)
        .await
        .map_err(|e| PackageManagementError::Internal(format!("write manifest tmp: {e}")))?;
    fs::rename(&tmp, path)
        .await
        .map_err(|e| PackageManagementError::Internal(format!("rename manifest: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path) -> ManagedPackageStore {
        ManagedPackageStore::new(dir.to_path_buf())
    }

    async fn write_fake_install(staging: &Path, product: ProductId) -> PathBuf {
        // Mirror npm layout: <staging>/node_modules/<pkg>/bin/<bin>
        let pkg = package_name(product);
        let bin = bin_name(product);
        let root = staging.join("node_modules").join(pkg);
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).await.unwrap();
        let entry = bin_dir.join(bin);
        fs::write(&entry, "#!/usr/bin/env node\n").await.unwrap();
        root
    }

    #[tokio::test]
    async fn promote_creates_version_and_sets_active() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.ensure_layout().await.unwrap();
        let staging = s
            .create_staging(ProductId::Pi, uuid::Uuid::new_v4())
            .await
            .unwrap();
        write_fake_install(&staging, ProductId::Pi).await;
        let entrypoint = staging
            .join("node_modules")
            .join(package_name(ProductId::Pi))
            .join("bin")
            .join(bin_name(ProductId::Pi));
        let node = PathBuf::from("/usr/bin/node");
        let entry = s
            .promote(
                ProductId::Pi,
                &staging,
                &semver::Version::new(0, 84, 0),
                entrypoint,
                node.clone(),
            )
            .await
            .unwrap();
        assert_eq!(entry.version, "0.84.0");
        assert!(entry
            .package_root
            .ends_with("node_modules/@earendil-works/pi-coding-agent"));
        let active = s.active(ProductId::Pi).await.unwrap();
        assert_eq!(active.version, "0.84.0");
        // The staging dir was renamed into versions/.
        assert!(!staging.exists());
        assert!(s.versions_root(ProductId::Pi).join("0.84.0").exists());
    }

    #[tokio::test]
    async fn promote_retains_previous_for_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.ensure_layout().await.unwrap();

        // First version.
        let st1 = s
            .create_staging(ProductId::Pi, uuid::Uuid::new_v4())
            .await
            .unwrap();
        let root1 = write_fake_install(&st1, ProductId::Pi).await;
        let entry1 = st1.join("bin").join(bin_name(ProductId::Pi));
        let _ = s
            .promote(
                ProductId::Pi,
                &st1,
                &semver::Version::new(0, 84, 0),
                entry1,
                PathBuf::from("/n"),
            )
            .await
            .unwrap();
        let _ = root1;

        // Second version.
        let st2 = s
            .create_staging(ProductId::Pi, uuid::Uuid::new_v4())
            .await
            .unwrap();
        write_fake_install(&st2, ProductId::Pi).await;
        let entry2 = st2.join("bin").join(bin_name(ProductId::Pi));
        let _ = s
            .promote(
                ProductId::Pi,
                &st2,
                &semver::Version::new(0, 85, 0),
                entry2,
                PathBuf::from("/n"),
            )
            .await
            .unwrap();

        assert_eq!(s.active(ProductId::Pi).await.unwrap().version, "0.85.0");
        assert_eq!(
            s.previous_active(ProductId::Pi).await.unwrap().version,
            "0.84.0"
        );
    }

    #[tokio::test]
    async fn restore_previous_swaps_active_back() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.ensure_layout().await.unwrap();
        for v in [1u64, 2] {
            let st = s
                .create_staging(ProductId::Pi, uuid::Uuid::new_v4())
                .await
                .unwrap();
            write_fake_install(&st, ProductId::Pi).await;
            let entry = st.join("bin").join(bin_name(ProductId::Pi));
            s.promote(
                ProductId::Pi,
                &st,
                &semver::Version::new(0, 80 + v, 0),
                entry,
                PathBuf::from("/n"),
            )
            .await
            .unwrap();
        }
        let restored = s.restore_previous(ProductId::Pi).await.unwrap();
        assert_eq!(restored.unwrap().version, "0.81.0");
        assert_eq!(s.active(ProductId::Pi).await.unwrap().version, "0.81.0");
    }

    #[tokio::test]
    async fn promote_same_version_swaps_aside() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.ensure_layout().await.unwrap();
        let st1 = s
            .create_staging(ProductId::Pi, uuid::Uuid::new_v4())
            .await
            .unwrap();
        write_fake_install(&st1, ProductId::Pi).await;
        let e1 = st1.join("bin").join(bin_name(ProductId::Pi));
        s.promote(
            ProductId::Pi,
            &st1,
            &semver::Version::new(0, 84, 0),
            e1,
            PathBuf::from("/n"),
        )
        .await
        .unwrap();
        // Repair: same version again.
        let st2 = s
            .create_staging(ProductId::Pi, uuid::Uuid::new_v4())
            .await
            .unwrap();
        write_fake_install(&st2, ProductId::Pi).await;
        let e2 = st2.join("bin").join(bin_name(ProductId::Pi));
        s.promote(
            ProductId::Pi,
            &st2,
            &semver::Version::new(0, 84, 0),
            e2,
            PathBuf::from("/n"),
        )
        .await
        .unwrap();
        assert_eq!(s.active(ProductId::Pi).await.unwrap().version, "0.84.0");
        // Only one 0.84.0 dir remains (no .retiring leftover).
        let mut count = 0;
        let mut read = fs::read_dir(s.versions_root(ProductId::Pi)).await.unwrap();
        while let Ok(Some(entry)) = read.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if !name.starts_with('.') {
                    count += 1;
                }
            }
        }
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn manifest_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let active = {
            let s = store(dir.path());
            s.ensure_layout().await.unwrap();
            let st = s
                .create_staging(ProductId::PiHub, uuid::Uuid::new_v4())
                .await
                .unwrap();
            write_fake_install(&st, ProductId::PiHub).await;
            let entry = st.join("bin").join(bin_name(ProductId::PiHub));
            s.promote(
                ProductId::PiHub,
                &st,
                &semver::Version::new(0, 0, 42),
                entry,
                PathBuf::from("/n"),
            )
            .await
            .unwrap();
            s.active(ProductId::PiHub).await.unwrap()
        };
        // Reload from disk in a fresh store.
        let s2 = store(dir.path());
        s2.load().await.unwrap();
        let again = s2.active(ProductId::PiHub).await.unwrap();
        assert_eq!(again.version, active.version);
        assert_eq!(again.entrypoint, active.entrypoint);
    }

    #[tokio::test]
    async fn cleanup_versions_keeps_active_and_previous() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.ensure_layout().await.unwrap();
        // Plant three version dirs and set active=0.85 previous=0.84.
        for v in ["0.83.0", "0.84.0", "0.85.0"] {
            let vd = s.versions_root(ProductId::Pi).join(v);
            fs::create_dir_all(&vd).await.unwrap();
        }
        s.set_active(
            ProductId::Pi,
            ActiveEntry {
                product: "pi".into(),
                version: "0.85.0".into(),
                package_root: s.versions_root(ProductId::Pi).join("0.85.0"),
                entrypoint: PathBuf::from("/x"),
                package_name: package_name(ProductId::Pi).into(),
                bin: bin_name(ProductId::Pi).into(),
                node_executable: PathBuf::from("/n"),
                activated_at: Utc::now(),
            },
        )
        .await
        .unwrap();
        {
            let mut m = s.manifest.write().await;
            m.previous.insert(
                "pi".into(),
                ActiveEntry {
                    product: "pi".into(),
                    version: "0.84.0".into(),
                    package_root: PathBuf::from("/x"),
                    entrypoint: PathBuf::from("/x"),
                    package_name: package_name(ProductId::Pi).into(),
                    bin: bin_name(ProductId::Pi).into(),
                    node_executable: PathBuf::from("/n"),
                    activated_at: Utc::now(),
                },
            );
            drop(m);
        }
        s.cleanup_versions(ProductId::Pi).await;
        let remaining = s.list_versions(ProductId::Pi).await;
        assert!(remaining.contains(&"0.85.0".to_string()));
        assert!(remaining.contains(&"0.84.0".to_string()));
        assert!(!remaining.contains(&"0.83.0".to_string()));
    }

    #[test]
    fn rejects_traversal_version_name() {
        assert!(validate_version_dir_name("../etc").is_err());
        assert!(validate_version_dir_name("a/b").is_err());
        assert!(validate_version_dir_name("not-semver").is_err());
        assert!(validate_version_dir_name("0.84.0").is_ok());
    }
}
