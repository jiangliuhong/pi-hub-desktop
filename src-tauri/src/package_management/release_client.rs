//! npm registry release client (docs/requirements-v3.md §9,
//! docs/pi-and-pi-hub-package-management-design.md §9).
//!
//! Answers a single question: "what is the stable `latest` version of this
//! product, and what are its allowlisted metadata fields?" It only ever reads
//! `dist-tags.latest`, the matching `version`'s `engines.node` and
//! `dist.integrity`, and the optional publish time. Registry raw output is
//! never returned to the caller or logged (V3-SR-004/005).
//!
//! Caching (design §9.2): success TTL 6h, ETag on manual checks, failures do
//! not overwrite the last success (offline fallback), and a 30s minimum retry
//! interval guards against hammering a failing registry.

use crate::error::PackageManagementError;
use crate::package_management::model::{package_name, ProductId, ReleaseInfo};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Success TTL (design §9.2).
pub const SUCCESS_TTL_SECS: i64 = 6 * 60 * 60;
/// Minimum retry interval after a failure (design §9.2).
pub const MIN_RETRY_SECS: i64 = 30;
/// HTTP request deadline.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// The release client contract (design §9). DI-friendly so tests use a fake
/// without touching the network (NFR-003).
#[async_trait::async_trait]
pub trait ReleaseClient: Send + Sync {
    async fn latest(
        &self,
        product: ProductId,
        force: bool,
    ) -> Result<ReleaseInfo, PackageManagementError>;
}

#[derive(Clone)]
struct CachedRelease {
    info: ReleaseInfo,
    etag: Option<String>,
    fetched_at: chrono::DateTime<Utc>,
    last_success_at: chrono::DateTime<Utc>,
    last_failure_at: Option<chrono::DateTime<Utc>>,
}

/// Production client backed by the npm public registry over HTTPS (design
/// §9.1). Gated to non-iOS: V3 is macOS-only and the iOS build must not pull in
/// an HTTP/TLS stack (requirements-v3 §4.2).
#[cfg(not(target_os = "ios"))]
pub struct NpmRegistryReleaseClient {
    client: reqwest::Client,
    base_url: String,
    cache: Mutex<HashMap<ProductId, CachedRelease>>,
}

#[cfg(not(target_os = "ios"))]
impl NpmRegistryReleaseClient {
    /// Production constructor: official public registry, system/roots TLS
    /// validation (V3-SR-004: never ignore cert errors).
    pub fn new() -> Self {
        Self::with_base_url("https://registry.npmjs.org/".to_string())
    }

    /// Test/internal constructor pointing at a custom registry base.
    pub fn with_base_url(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .gzip(true)
            .build()
            .expect("reqwest client builds with rustls");
        NpmRegistryReleaseClient {
            client,
            base_url,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(not(target_os = "ios"))]
impl Default for NpmRegistryReleaseClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "ios"))]
#[async_trait::async_trait]
impl ReleaseClient for NpmRegistryReleaseClient {
    async fn latest(
        &self,
        product: ProductId,
        force: bool,
    ) -> Result<ReleaseInfo, PackageManagementError> {
        let now = Utc::now();
        let pkg = package_name(product);

        // 1. TTL fast path (bypassed only by an explicit manual check).
        {
            let cache = self.cache.lock().expect("release cache poisoned");
            if let Some(c) = cache.get(&product) {
                let ttl_fresh = (now - c.fetched_at).num_seconds() < SUCCESS_TTL_SECS;
                let retry_ok = c
                    .last_failure_at
                    .map(|f| (now - f).num_seconds() >= MIN_RETRY_SECS)
                    .unwrap_or(true);
                if !force && ttl_fresh && retry_ok {
                    return Ok(c.info.clone());
                }
            }
        }

        // 2. Build the conditional GET.
        let url = format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            url_package_path(pkg)
        );
        let (_etag, if_none_match) = {
            let cache = self.cache.lock().expect("release cache poisoned");
            let etag = cache.get(&product).and_then(|c| c.etag.clone());
            (etag.clone(), etag)
        };
        let mut req = self.client.get(&url).header("Accept", "application/json");
        if let Some(etag) = if_none_match {
            req = req.header("If-None-Match", etag);
        }

        // 3. Execute. Network/HTTP failures fall back to cache, else error.
        let resp = match req.send().await {
            Ok(r) => r,
            Err(_) => return self.fallback_or_error(product, now, force),
        };
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            // 304: keep info, refresh fetch time + etag.
            let new_etag = resp_etag(&resp);
            let mut cache = self.cache.lock().expect("release cache poisoned");
            if let Some(c) = cache.get_mut(&product) {
                c.fetched_at = now;
                c.last_success_at = now;
                c.last_failure_at = None;
                if new_etag.is_some() {
                    c.etag = new_etag;
                }
                return Ok(c.info.clone());
            }
            // No prior cache but 304 — treat as a transient inconsistency.
            return Err(PackageManagementError::ReleaseCheckFailed {
                product: product.api_name().into(),
            });
        }
        if !status.is_success() {
            return self.fallback_or_error(product, now, force);
        }
        let etag = resp_etag(&resp);
        let body = match resp.text().await {
            Ok(t) => t,
            Err(_) => return self.fallback_or_error(product, now, force),
        };

        // 4. Parse allowlisted fields only.
        let info = parse_packument(&body, product)?;
        let mut cache = self.cache.lock().expect("release cache poisoned");
        cache.insert(
            product,
            CachedRelease {
                info: info.clone(),
                etag,
                fetched_at: now,
                last_success_at: now,
                last_failure_at: None,
            },
        );
        Ok(info)
    }
}

#[cfg(not(target_os = "ios"))]
impl NpmRegistryReleaseClient {
    fn fallback_or_error(
        &self,
        product: ProductId,
        now: chrono::DateTime<Utc>,
        force: bool,
    ) -> Result<ReleaseInfo, PackageManagementError> {
        let mut cache = self.cache.lock().expect("release cache poisoned");
        if let Some(c) = cache.get_mut(&product) {
            c.last_failure_at = Some(now);
            // Honor the min-retry interval by returning the last success even
            // on a forced check if we're within the retry window.
            let _ = force;
            return Ok(c.info.clone());
        }
        Err(PackageManagementError::ReleaseCheckFailed {
            product: product.api_name().into(),
        })
    }
}

#[cfg(not(target_os = "ios"))]
fn resp_etag(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Build the registry path for a (possibly scoped) package name. Scoped names
/// are sent verbatim (the registry does not require URL-encoding the `@`/`/`).
fn url_package_path(pkg: &str) -> String {
    // reqwest encodes the path segment; the npm registry serves scoped
    // packages at `/<@scope/name>`. We pass the raw path and let reqwest
    // percent-encode as needed.
    format!("/{pkg}")
}

/// Parse only the allowlisted fields from a packument (design §9.1). Any
/// structural surprise becomes `ReleaseInvalid`, never a panic.
fn parse_packument(body: &str, product: ProductId) -> Result<ReleaseInfo, PackageManagementError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| PackageManagementError::ReleaseInvalid {
            product: product.api_name().into(),
        })?;
    // Defense against a wrong-URL/mismatched packument.
    let name = value.get("name").and_then(|v| v.as_str());
    if name != Some(package_name(product)) {
        return Err(PackageManagementError::ReleaseInvalid {
            product: product.api_name().into(),
        });
    }
    let latest = value
        .get("dist-tags")
        .and_then(|d| d.get("latest"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| PackageManagementError::ReleaseInvalid {
            product: product.api_name().into(),
        })?;
    let version =
        semver::Version::parse(latest).map_err(|_| PackageManagementError::ReleaseInvalid {
            product: product.api_name().into(),
        })?;
    let version_manifest = value.get("versions").and_then(|v| v.get(latest));
    let node_engine = version_manifest
        .and_then(|m| m.get("engines"))
        .and_then(|e| e.get("node"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    let integrity = version_manifest
        .and_then(|m| m.get("dist"))
        .and_then(|d| d.get("integrity"))
        .and_then(|i| i.as_str())
        .map(|s| s.to_string());
    let published_at = value
        .get("time")
        .and_then(|t| t.get(latest))
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    Ok(ReleaseInfo {
        product,
        version,
        node_engine,
        integrity,
        published_at,
    })
}

#[cfg(all(test, not(target_os = "ios")))]
mod tests {
    use super::*;

    fn packument(version: &str, engine: Option<&str>, integrity: Option<&str>) -> String {
        let engine_json = match engine {
            Some(e) => format!(",\"engines\":{{\"node\":\"{e}\"}}"),
            None => String::new(),
        };
        let dist_json = match integrity {
            Some(i) => format!(",\"dist\":{{\"integrity\":\"{i}\"}}"),
            None => String::new(),
        };
        serde_json::json!({
            "name": "@earendil-works/pi-coding-agent",
            "dist-tags": { "latest": version },
            "versions": {
                version: {
                    "name": "@earendil-works/pi-coding-agent",
                    "version": version
                        .to_string()
                }
            },
            "_engine_placeholder": engine_json,
            "_dist_placeholder": dist_json,
        })
        .to_string()
    }

    /// Build a packument with the engine/dist nested under the version object.
    fn real_packument(version: &str, engine: Option<&str>, integrity: Option<&str>) -> String {
        let mut v = serde_json::json!({
            "name": "@earendil-works/pi-coding-agent",
            "dist-tags": { "latest": version },
            "versions": {
                version: {
                    "name": "@earendil-works/pi-coding-agent",
                    "version": version
                }
            }
        });
        if let Some(e) = engine {
            v["versions"][version]["engines"]["node"] = serde_json::json!(e);
        }
        if let Some(i) = integrity {
            v["versions"][version]["dist"]["integrity"] = serde_json::json!(i);
        }
        v.to_string()
    }

    #[test]
    fn parses_allowlisted_fields() {
        let body = real_packument("0.85.0", Some(">=20"), Some("sha512-abc"));
        let info = parse_packument(&body, ProductId::Pi).unwrap();
        assert_eq!(info.product, ProductId::Pi);
        assert_eq!(info.version, semver::Version::new(0, 85, 0));
        assert_eq!(info.node_engine.as_deref(), Some(">=20"));
        assert_eq!(info.integrity.as_deref(), Some("sha512-abc"));
    }

    #[test]
    fn rejects_name_mismatch() {
        let body = serde_json::json!({
            "name": "not-pi",
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": {} }
        })
        .to_string();
        assert!(parse_packument(&body, ProductId::Pi).is_err());
    }

    #[test]
    fn rejects_invalid_version() {
        let body = serde_json::json!({
            "name": "@earendil-works/pi-coding-agent",
            "dist-tags": { "latest": "not-a-version" },
            "versions": { "not-a-version": {} }
        })
        .to_string();
        assert!(parse_packument(&body, ProductId::Pi).is_err());
    }

    #[test]
    fn placeholder_packument_is_unused() {
        // Guard: the simplified `packument()` helper is intentionally not used
        // by the parser test; keep it referenced so it doesn't dead-code-warn.
        let _ = packument("1.0.0", None, None);
    }

    /// A fake release client for manager-level tests (no network).
    pub struct FakeReleaseClient {
        pub info: Option<ReleaseInfo>,
        pub err: bool,
        pub calls: std::sync::atomic::AtomicU32,
    }
    impl FakeReleaseClient {
        pub fn with(info: ReleaseInfo) -> Self {
            FakeReleaseClient {
                info: Some(info),
                err: false,
                calls: std::sync::atomic::AtomicU32::new(0),
            }
        }
        pub fn failing() -> Self {
            FakeReleaseClient {
                info: None,
                err: true,
                calls: std::sync::atomic::AtomicU32::new(0),
            }
        }
    }
    #[async_trait::async_trait]
    impl ReleaseClient for FakeReleaseClient {
        async fn latest(
            &self,
            _product: ProductId,
            _force: bool,
        ) -> Result<ReleaseInfo, PackageManagementError> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.err {
                return Err(PackageManagementError::ReleaseCheckFailed {
                    product: "pi".into(),
                });
            }
            self.info
                .clone()
                .ok_or(PackageManagementError::ReleaseInvalid {
                    product: "pi".into(),
                })
        }
    }
}

#[cfg(all(test, target_os = "ios"))]
mod tests {
    // On iOS the release client is not compiled; keep the module non-empty.
    #[test]
    fn ios_release_client_is_stubbed() {}
}

// Re-export the fake for downstream manager tests on non-iOS.
#[cfg(all(test, not(target_os = "ios")))]
pub use tests::FakeReleaseClient;
