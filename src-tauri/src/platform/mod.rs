//! Platform adaptation (docs/design-v1.md §4, §18).
//!
//! Shared domain logic lives in the domain modules; only genuine macOS / iOS
//! differences are handled here. No fake background modes (audio/location) to
//! keep tunnels alive (AGENTS.md §8.2).
//!
//! V1 lifecycle hooks (foreground/background reconnect, app-terminate resource
//! release) are wired in the lifecycle phase and validated on real devices
//! (AGENTS.md §12.4).

use crate::error::PlatformError;

/// Where the app currently is in the mobile lifecycle. Desktop is always
/// `Foreground` for the purposes of reconnect policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLifecycle {
    Foreground,
    Background,
}

/// Return the platform default profile store path inside the app's private
/// config directory. On Apple this resolves to Application Support / app
/// container; the path is supplied by the caller in practice (Tauri path
/// plugin), but this helper documents the contract.
pub fn default_store_filename() -> &'static str {
    "pihub-store.json"
}

/// Whether automatic reconnect should be attempted right now given the
/// lifecycle (design §18.2). iOS background never auto-reconnects aggressively.
pub fn should_auto_reconnect(lifecycle: AppLifecycle) -> Result<bool, PlatformError> {
    Ok(matches!(lifecycle, AppLifecycle::Foreground))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_foreground_auto_reconnects() {
        assert!(should_auto_reconnect(AppLifecycle::Foreground).unwrap());
        assert!(!should_auto_reconnect(AppLifecycle::Background).unwrap());
    }
}
