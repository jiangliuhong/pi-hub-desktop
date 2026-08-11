//! Forward schema migrations (docs/design-v1.md §11, AGENTS.md §11).
//!
//! Every structural change adds an explicit forward migration. We never clear
//! user configuration as a "migration". Version 1 is the baseline; future
//! versions append `migrate_v1_to_v2`, etc.

use crate::error::ProfileError;
use serde_json::Value;

/// Apply forward migrations to a raw store value until it reaches the current
/// schema version. The input must be the parsed JSON document of the store
/// file (a JSON object). Returns the migrated value.
pub fn migrate_to_current(value: Value) -> Result<Value, ProfileError> {
    let version = current_version(&value)?;
    if version > super::model::CURRENT_SCHEMA_VERSION {
        return Err(ProfileError::Migration(format!(
            "stored schema_version {version} is newer than supported {}",
            super::model::CURRENT_SCHEMA_VERSION
        )));
    }

    // No real migrations yet (v1 baseline). Each future step takes the form:
    //   if version < 2 { value = migrate_v1_to_v2(value)?; }
    // Keep the dispatch explicit and tested so a future bump is mechanical.
    let _ = version;
    Ok(value)
}

fn current_version(value: &Value) -> Result<u32, ProfileError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ProfileError::Migration("store root is not an object".into()))?;
    let raw = obj
        .get("schema_version")
        .ok_or_else(|| ProfileError::Migration("missing schema_version".into()))?;
    let v = raw
        .as_u64()
        .ok_or_else(|| ProfileError::Migration("schema_version is not an integer".into()))?;
    u32::try_from(v).map_err(|_| ProfileError::Migration("schema_version overflows u32".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_current_version() {
        let v = serde_json::json!({
            "schema_version": 1,
            "profiles": [],
            "known_hosts": []
        });
        let out = migrate_to_current(v.clone()).expect("ok");
        assert_eq!(out, v);
    }

    #[test]
    fn rejects_newer_version() {
        let v = serde_json::json!({ "schema_version": 999, "profiles": [] });
        assert!(migrate_to_current(v).is_err());
    }

    #[test]
    fn rejects_non_object_root() {
        let v = serde_json::json!([1, 2, 3]);
        assert!(migrate_to_current(v).is_err());
    }

    #[test]
    fn rejects_missing_version() {
        let v = serde_json::json!({ "profiles": [] });
        assert!(migrate_to_current(v).is_err());
    }
}
