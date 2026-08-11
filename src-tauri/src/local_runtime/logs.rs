//! In-memory runtime log ring buffer (docs/design-v2.md §15.1).
//!
//! Captures a bounded window of the managed Pi Hub's stdout/stderr. Lines are
//! timestamped by the Desktop, capped in length and count, and run through the
//! redactor before they ever reach the buffer (design-v2 §15.3, V2-SR-004).
//! The buffer holds no secrets and never streams raw output to the frontend.

use crate::local_runtime::model::{LogLine, LogStream};
use crate::local_runtime::redaction::redact_line;
use chrono::Utc;
use std::collections::VecDeque;
use std::sync::Mutex;

/// Default ring-buffer sizes (design-v2 §15.1).
pub const DEFAULT_MAX_LINES: usize = 500;
pub const DEFAULT_MAX_LINE_BYTES: usize = 16 * 1024;
/// Default number of recent lines surfaced to the UI.
pub const DEFAULT_UI_LIMIT: usize = 200;

/// A bounded, redacting ring buffer of captured child output.
pub struct RuntimeLogBuffer {
    inner: Mutex<Inner>,
}

struct Inner {
    lines: VecDeque<LogLine>,
    max_lines: usize,
    max_line_bytes: usize,
}

impl Default for RuntimeLogBuffer {
    fn default() -> Self {
        RuntimeLogBuffer::new(DEFAULT_MAX_LINES, DEFAULT_MAX_LINE_BYTES)
    }
}

impl RuntimeLogBuffer {
    pub fn new(max_lines: usize, max_line_bytes: usize) -> Self {
        RuntimeLogBuffer {
            inner: Mutex::new(Inner {
                lines: VecDeque::with_capacity(max_lines.min(1024)),
                max_lines,
                max_line_bytes,
            }),
        }
    }

    /// Append a raw chunk of output from the given stream. The chunk may
    /// contain multiple lines; each is split, redacted, length-capped and
    /// timestamped. Trailing partial lines (no newline) are still recorded.
    pub fn push_raw(&self, stream: LogStream, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let cap = inner.max_line_bytes;
        let now = Utc::now();
        for raw in chunk.split('\n') {
            let redacted = redact_line(raw);
            let text = truncate_bytes(&redacted, cap);
            let line = LogLine {
                timestamp: now,
                stream,
                text,
            };
            if inner.lines.len() >= inner.max_lines {
                inner.lines.pop_front();
            }
            inner.lines.push_back(line);
        }
    }

    /// Snapshot the most recent `limit` lines (oldest→newest). `None` returns
    /// all buffered lines.
    pub fn recent(&self, limit: Option<usize>) -> Vec<LogLine> {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let take = match limit {
            Some(n) => n.min(inner.lines.len()),
            None => inner.lines.len(),
        };
        inner
            .lines
            .iter()
            .rev()
            .take(take)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Total number of buffered lines.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|i| i.lines.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop all buffered lines (e.g. before a fresh managed start).
    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.lines.clear();
        }
    }
}

/// Truncate to at most `max_bytes` UTF-8 boundary-safe, appending an ellipsis
/// indicator so truncation is visible (and never producing invalid UTF-8).
fn truncate_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 3);
    out.push_str(&input[..end]);
    out.push_str("…[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_and_reads_lines() {
        let buf = RuntimeLogBuffer::default();
        buf.push_raw(LogStream::Stdout, "hello\nworld");
        let recent = buf.recent(None);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].text, "hello");
        assert_eq!(recent[1].text, "world");
    }

    #[test]
    fn caps_line_count_oldest_dropped() {
        let buf = RuntimeLogBuffer::new(3, 1024);
        buf.push_raw(LogStream::Stdout, "a\nb\nc\nd");
        let recent = buf.recent(None);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].text, "b");
        assert_eq!(recent[2].text, "d");
    }

    #[test]
    fn redacts_before_buffering() {
        let buf = RuntimeLogBuffer::default();
        buf.push_raw(LogStream::Stderr, "OPENAI_API_KEY=sk-leaked");
        let line = &buf.recent(None)[0];
        assert!(line.text.contains("[REDACTED]"));
        assert!(!line.text.contains("sk-leaked"));
    }

    #[test]
    fn truncates_overlong_lines_at_char_boundary() {
        let buf = RuntimeLogBuffer::new(10, 10);
        // "é" is two bytes; ensure we land on a boundary.
        buf.push_raw(LogStream::Stdout, "éééééééééé");
        let line = &buf.recent(None)[0];
        assert!(line.text.ends_with("…[truncated]"));
        assert!(line.text.is_char_boundary(line.text.len()));
    }

    #[test]
    fn recent_limit_returns_newest() {
        let buf = RuntimeLogBuffer::new(10, 1024);
        buf.push_raw(LogStream::Stdout, "1\n2\n3\n4\n5");
        let recent = buf.recent(Some(2));
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].text, "4");
        assert_eq!(recent[1].text, "5");
    }

    #[test]
    fn clear_empties_buffer() {
        let buf = RuntimeLogBuffer::default();
        buf.push_raw(LogStream::Stdout, "x");
        assert!(!buf.is_empty());
        buf.clear();
        assert!(buf.is_empty());
    }
}
