//! SSH client and Local Port Forward (docs/design-v1.md §7.3, §8).
//!
//! Built on `russh` with `direct-tcpip`. Intended internal modules:
//! - `client.rs` — session lifecycle, keepalive, cancellation
//! - `host_key.rs` — fingerprint, known-host compare, first-time challenge
//! - `key_loader.rs` — OpenSSH Ed25519/RSA + passphrase parsing
//! - `forward.rs` — loopback `TcpListener` (127.0.0.1:0) + accept loop and
//!   bidirectional `direct-tcpip` copy
//!
//! Hard rules (AGENTS.md §6.2, §6.3, §7): strict host-key checking, loopback
//! only, system-assigned ephemeral port, never log payload. This module is a
//! stub until V1 Phase 2.
