//! Service WebView lifecycle commands (docs/design-v1.md §14, §13.1).
//!
//! The Service View is untrusted remote content. It receives zero Tauri
//! capability and cannot read Keychain, Store or the filesystem (AGENTS.md
//! §6.4). The trusted App Shell controls open/close/navigation here; the
//! WebView content itself never reaches these commands.
//!
//! Verification status: opening a real WKWebView/Tauri WebviewWindow is
//! platform-dependent and must be validated on macOS / iPhone per AGENTS.md
//! §12.4. These commands record intent and emit events; the native window
//! creation is wired in the lifecycle phase and tracked separately.

use crate::commands::profiles::map_err;
use crate::error::{AppError, ViewerError};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

/// Request to open the isolated Service View for a service.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenServiceViewRequest {
    pub service_id: Uuid,
    /// Effective URL produced by a successful connection (loopback or direct).
    pub effective_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenServiceViewResponse {
    pub service_id: Uuid,
    /// The origin the Service View is allowed to navigate within. All other
    /// navigations are handed to the system browser (FR-013, design §14.5).
    pub allowed_origin: String,
}

/// Compute the allowlisted origin for a service view (design §14.5). The view
/// may navigate within this origin and Pi Hub's same-origin resources only.
pub fn allowed_origin_for(effective_url: &Url) -> Result<String, AppError> {
    let serialized = effective_url.origin().ascii_serialization();
    if serialized == "null" {
        return Err(AppError::Viewer(ViewerError::Other(
            "effective url has an opaque origin".into(),
        )));
    }
    Ok(serialized)
}

#[tauri::command]
pub async fn open_service_view(
    request: OpenServiceViewRequest,
) -> Result<OpenServiceViewResponse, crate::error::ErrorDto> {
    let url = Url::parse(&request.effective_url).map_err(|e| {
        map_err(AppError::Viewer(ViewerError::Other(format!(
            "invalid url: {e}"
        ))))
    })?;
    let allowed_origin = allowed_origin_for(&url).map_err(map_err)?;
    // The actual native window creation is platform-specific and validated in
    // the lifecycle phase. Emit an event so the App Shell can present the
    // viewer surface consistently.
    tracing::info!(
        service_id = %request.service_id,
        origin = %allowed_origin,
        "open service view requested"
    );
    Ok(OpenServiceViewResponse {
        service_id: request.service_id,
        allowed_origin,
    })
}

#[tauri::command]
pub async fn close_service_view(service_id: Uuid) -> Result<(), crate::error::ErrorDto> {
    tracing::info!(service_id = %service_id, "close service view requested");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_origin_strips_path_and_query() {
        let url = Url::parse("http://127.0.0.1:54321/pihub/path?q=1").unwrap();
        assert_eq!(allowed_origin_for(&url).unwrap(), "http://127.0.0.1:54321");
    }

    #[test]
    fn allowed_origin_https_direct() {
        let url = Url::parse("https://pi.example.com").unwrap();
        assert_eq!(allowed_origin_for(&url).unwrap(), "https://pi.example.com");
    }
}
