//! Structured event payloads (docs/design-v1.md §13.2).
//!
//! Events pushed to the Trusted App Shell:
//! - `connection://state-changed`
//! - `connection://diagnostics-updated`
//! - `ssh://host-key-challenge`
//! - `viewer://closed`
//! - `app://foregrounded`
//! - `app://backgrounded`
//!
//! Event payloads must never contain credentials, Authorization, cookies or
//! page content (AGENTS.md §6.1, §6.4). This module is a stub until V1
//! Phase 1/2.
