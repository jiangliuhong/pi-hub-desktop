//! Connection-state broadcaster (plan-remote-pi-hub-performance §5.5).
//!
//! The connection layer is constructed early, before the Tauri builder runs,
//! so it cannot capture the `AppHandle` at construction time (unlike the V2
//! `local_runtime` and V3 `package_management` managers). Instead we mirror the
//! same dependency-injection seam — a `ConnectionBroadcaster` trait with a
//! `Noop` default for tests and a `Tauri` impl injected in `.setup` — and add a
//! `set_broadcaster` setter so `lib.rs` can hand the handle over once it
//! exists.
//!
//! Events emitted here target the trusted App Shell only. The Service View
//! WebView has no matching capability and can never receive them (§6.4).

use crate::event::{DiagnosticsPayload, StateChangedPayload};
use async_trait::async_trait;
use uuid::Uuid;

/// Event names (docs/design-v1.md §13.2).
pub const STATE_CHANGED_EVENT: &str = "connection://state-changed";
pub const DIAGNOSTICS_UPDATED_EVENT: &str = "connection://diagnostics-updated";

/// Pushes non-sensitive connection state / diagnostics events to the App Shell.
///
/// All methods take pre-built, secret-free payloads; the broadcaster never
/// inspects or redacts content — that responsibility lives with the callers
/// (see `diagnostics.rs`, `event.rs`).
#[async_trait]
pub trait ConnectionBroadcaster: Send + Sync {
    async fn broadcast_state(&self, payload: StateChangedPayload);
    async fn broadcast_diagnostics(&self, payload: DiagnosticsPayload);
}

/// No-op implementation used by tests and during early construction (before
/// the Tauri handle is available).
pub struct NoopBroadcaster;

#[async_trait]
impl ConnectionBroadcaster for NoopBroadcaster {
    async fn broadcast_state(&self, _payload: StateChangedPayload) {}
    async fn broadcast_diagnostics(&self, _payload: DiagnosticsPayload) {}
}

/// Tauri-backed broadcaster. Emits to the trusted App Shell window only.
pub struct TauriBroadcaster {
    handle: tauri::AppHandle,
}

impl TauriBroadcaster {
    pub fn new(handle: tauri::AppHandle) -> Self {
        TauriBroadcaster { handle }
    }
}

#[async_trait]
impl ConnectionBroadcaster for TauriBroadcaster {
    async fn broadcast_state(&self, payload: StateChangedPayload) {
        use tauri::Emitter;
        // Payload carries only service_id, a state enum string and an
        // effective URL (loopback host:port). No secrets traverse this.
        let _ = self.handle.emit(STATE_CHANGED_EVENT, payload);
    }

    async fn broadcast_diagnostics(&self, payload: DiagnosticsPayload) {
        use tauri::Emitter;
        let _ = self.handle.emit(DIAGNOSTICS_UPDATED_EVENT, payload);
    }
}

/// Helper to build a state-changed payload from the connection fields the
/// manager already tracks. Kept here so the manager call sites stay terse and
/// the payload shape lives next to the event name.
pub fn state_changed(
    service_id: Uuid,
    state: &str,
    effective_url: Option<&str>,
) -> StateChangedPayload {
    StateChangedPayload {
        service_id,
        state: state.to_string(),
        effective_url: effective_url.map(|s| s.to_string()),
    }
}
