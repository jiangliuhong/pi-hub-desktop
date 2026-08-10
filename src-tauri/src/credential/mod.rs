//! Credential storage abstraction (docs/design-v1.md §10).
//!
//! Secrets live only in Apple Keychain; profiles hold references. Intended
//! internal modules:
//! - `mod.rs`           — `CredentialStore` trait, `CredentialId`, `SecretValue`
//! - `apple_keychain.rs`— `security-framework` backed implementation
//!
//! Keychain item service is `APP_BUNDLE_ID`; account is
//! `credential/<uuid>/<kind>` (design §6.2). This module is a stub until
//! V1 Phase 1.
