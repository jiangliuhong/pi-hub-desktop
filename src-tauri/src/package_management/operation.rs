//! Operation tracking, bounded sanitized logging and cancellation
//! (docs/requirements-v3.md §13; design §13, §18).
//!
//! - `OperationLogBuffer`: a fixed-capacity ring of sanitized log lines per
//!   operation (design §18: bounded lines + bytes, redacted).
//! - `OperationHandle`: the in-flight operation record the manager owns; it
//!   carries the cancellation token, current stage and the log buffer.

use crate::package_management::model::{
    PackageLogLevel, PackageOperationKind, PackageOperationStage,
};
use chrono::Utc;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Max lines retained per operation (design §18).
pub const MAX_LOG_LINES: usize = 500;
/// Max bytes per line (longer lines are truncated).
pub const MAX_LOG_LINE_BYTES: usize = 4_096;
/// How many recent operations are retained on disk/in-memory (design §18).
pub const MAX_RECENT_OPERATIONS: usize = 10;

/// A sink for sanitized operation log lines (DI so the installer doesn't own
/// the buffer).
pub trait OperationLogSink: Send + Sync {
    fn push(&self, stage: PackageOperationStage, level: PackageLogLevel, text: &str);
}

/// Bounded, sanitized ring buffer for one operation's log lines (design §18).
pub struct OperationLogBuffer {
    lines: Mutex<VecDeque<crate::package_management::model::PackageOperationLogLine>>,
}

impl Default for OperationLogBuffer {
    fn default() -> Self {
        OperationLogBuffer {
            lines: Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES)),
        }
    }
}

impl OperationLogBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a (already-sanitized) line. Truncates long lines and drops the
    /// oldest when over capacity.
    pub fn push_line(&self, stage: PackageOperationStage, level: PackageLogLevel, text: &str) {
        let truncated = truncate_bytes(text, MAX_LOG_LINE_BYTES);
        let mut lines = self.lines.lock().expect("op log poisoned");
        if lines.len() >= MAX_LOG_LINES {
            lines.pop_front();
        }
        lines.push_back(crate::package_management::model::PackageOperationLogLine {
            timestamp: Utc::now(),
            stage,
            level,
            text: truncated,
        });
    }

    pub fn recent(
        &self,
        limit: Option<usize>,
    ) -> Vec<crate::package_management::model::PackageOperationLogLine> {
        let lines = self.lines.lock().expect("op log poisoned");
        let n = limit.unwrap_or(MAX_LOG_LINES).min(lines.len());
        lines.iter().rev().take(n).rev().cloned().collect()
    }
}

impl OperationLogSink for OperationLogBuffer {
    fn push(&self, stage: PackageOperationStage, level: PackageLogLevel, text: &str) {
        self.push_line(stage, level, text);
    }
}

/// Truncate to at most `max_bytes` (UTF-8 boundary), reserving room for the
/// ellipsis so the result never exceeds `max_bytes`.
fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    const ELLIPSIS_BYTES: usize = 3; // "…" is 3 UTF-8 bytes
    let target = max_bytes.saturating_sub(ELLIPSIS_BYTES);
    let mut end = target;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + ELLIPSIS_BYTES);
    out.push_str(&s[..end]);
    out.push('…');
    out
}

/// The in-flight operation record owned by the manager (design §13.1).
pub struct OperationHandle {
    pub id: uuid::Uuid,
    pub product: crate::package_management::model::ProductId,
    pub kind: PackageOperationKind,
    pub started_at: chrono::DateTime<Utc>,
    from_version: Mutex<Option<String>>,
    target_version: Mutex<Option<String>>,
    pub cancel: CancellationToken,
    pub log: std::sync::Arc<OperationLogBuffer>,
    stage: Mutex<PackageOperationStage>,
}

impl OperationHandle {
    pub fn new(
        product: crate::package_management::model::ProductId,
        kind: PackageOperationKind,
        from_version: Option<String>,
        target_version: Option<String>,
    ) -> Self {
        OperationHandle {
            id: uuid::Uuid::new_v4(),
            product,
            kind,
            started_at: Utc::now(),
            from_version: Mutex::new(from_version),
            target_version: Mutex::new(target_version),
            cancel: CancellationToken::new(),
            log: std::sync::Arc::new(OperationLogBuffer::new()),
            stage: Mutex::new(PackageOperationStage::Preparing),
        }
    }

    pub fn stage(&self) -> PackageOperationStage {
        *self.stage.lock().expect("op stage poisoned")
    }

    pub fn set_stage(&self, stage: PackageOperationStage) {
        *self.stage.lock().expect("op stage poisoned") = stage;
    }

    pub fn from_version(&self) -> Option<String> {
        self.from_version
            .lock()
            .expect("op from_version poisoned")
            .clone()
    }

    pub fn set_target_version(&self, v: Option<String>) {
        *self
            .target_version
            .lock()
            .expect("op target_version poisoned") = v;
    }

    pub fn target_version(&self) -> Option<String> {
        self.target_version
            .lock()
            .expect("op target_version poisoned")
            .clone()
    }

    /// DTO for the frontend.
    pub fn to_dto(&self) -> crate::package_management::model::PackageOperationDto {
        crate::package_management::model::PackageOperationDto {
            operation_id: self.id,
            product: self.product,
            kind: self.kind,
            stage: self.stage(),
            from_version: self.from_version(),
            target_version: self.target_version(),
            started_at: self.started_at,
            can_cancel: self.stage().cancellable(),
            issue: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_caps_line_count() {
        let buf = OperationLogBuffer::new();
        for i in 0..(MAX_LOG_LINES + 50) {
            buf.push_line(
                PackageOperationStage::Installing,
                PackageLogLevel::Info,
                &format!("line {i}"),
            );
        }
        assert_eq!(buf.recent(None).len(), MAX_LOG_LINES);
    }

    #[test]
    fn buffer_truncates_long_lines() {
        let buf = OperationLogBuffer::new();
        let long = "a".repeat(MAX_LOG_LINE_BYTES + 100);
        buf.push_line(
            PackageOperationStage::Installing,
            PackageLogLevel::Info,
            &long,
        );
        let line = &buf.recent(None)[0].text;
        assert!(line.ends_with('…'));
        assert!(line.len() <= MAX_LOG_LINE_BYTES + 1);
    }

    #[test]
    fn handle_stage_round_trips() {
        let h = OperationHandle::new(
            crate::package_management::model::ProductId::Pi,
            PackageOperationKind::Install,
            None,
            Some("0.85.0".into()),
        );
        assert_eq!(h.stage(), PackageOperationStage::Preparing);
        h.set_stage(PackageOperationStage::Installing);
        assert_eq!(h.stage(), PackageOperationStage::Installing);
        assert!(h.to_dto().can_cancel);
    }
}
