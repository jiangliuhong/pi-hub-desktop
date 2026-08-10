//! Platform adaptation (docs/design-v1.md §4, §18).
//!
//! Shared domain logic lives in the modules above; only genuine macOS / iOS
//! differences are handled here. Intended internal modules:
//! - `mod.rs`  — cross-platform traits / dispatch
//! - `macos.rs`— macOS window + app-terminate resource release
//! - `ios.rs`  — iOS foreground/background reconnect policy
//!
//! No fake background modes (audio/location) to keep tunnels alive
//! (AGENTS.md §8.2). This module is a stub until the lifecycle phase.
