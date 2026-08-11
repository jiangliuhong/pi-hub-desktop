//! Connection state machine (docs/design-v1.md §12.3, FR-009, AGENTS.md §9).
//!
//! States are an explicit enum — never scattered strings. Illegal transitions
//! are rejected and covered by unit tests (design §22.1).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Connection lifecycle states (AGENTS.md §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Idle,
    Validating,
    ConnectingSsh,
    VerifyingHostKey,
    Authenticating,
    OpeningForward,
    CheckingService,
    Connected,
    Reconnecting,
    Disconnecting,
    Disconnected,
    Error,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.api_name())
    }
}

impl ConnectionState {
    /// Stable snake_case wire name (mirrors the serde rename and the frontend
    /// union, design §9).
    pub fn api_name(self) -> &'static str {
        match self {
            ConnectionState::Idle => "idle",
            ConnectionState::Validating => "validating",
            ConnectionState::ConnectingSsh => "connecting_ssh",
            ConnectionState::VerifyingHostKey => "verifying_host_key",
            ConnectionState::Authenticating => "authenticating",
            ConnectionState::OpeningForward => "opening_forward",
            ConnectionState::CheckingService => "checking_service",
            ConnectionState::Connected => "connected",
            ConnectionState::Reconnecting => "reconnecting",
            ConnectionState::Disconnecting => "disconnecting",
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Error => "error",
        }
    }

    /// Whether the state represents an active (in-progress or connected)
    /// connection for which resources may be held.
    pub fn is_active(self) -> bool {
        !matches!(
            self,
            ConnectionState::Idle | ConnectionState::Disconnected | ConnectionState::Error
        )
    }

    /// Returns the legal successor for `self`, or `None` for terminal-ish
    /// states that must route through `disconnecting`.
    pub fn legal_successors(self) -> &'static [ConnectionState] {
        use ConnectionState::*;
        match self {
            Idle => &[Validating],
            Validating => &[
                ConnectingSsh,
                VerifyingHostKey,
                CheckingService,
                Error,
                Disconnecting,
            ],
            ConnectingSsh => &[VerifyingHostKey, Error, Disconnecting],
            VerifyingHostKey => &[Authenticating, Error, Disconnecting],
            Authenticating => &[OpeningForward, Error, Disconnecting],
            OpeningForward => &[CheckingService, Error, Disconnecting],
            CheckingService => &[Connected, Error, Disconnecting],
            Connected => &[Reconnecting, Error, Disconnecting],
            Reconnecting => &[CheckingService, Connected, Error, Disconnecting],
            // `disconnecting` / `disconnected` / `error` are terminal for the
            // active flow; recovery re-enters at Idle/Validating on a new run.
            Disconnecting => &[Disconnected],
            Disconnected => &[],
            Error => &[Validating, Idle],
        }
    }

    /// Validate a transition. Returns the next state on success.
    pub fn transition(self, next: ConnectionState) -> Result<ConnectionState, StateError> {
        if self.legal_successors().contains(&next) {
            Ok(next)
        } else {
            Err(StateError {
                from: self,
                to: next,
            })
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("illegal state transition: {from:?} -> {to:?}")]
pub struct StateError {
    pub from: ConnectionState,
    pub to: ConnectionState,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn happy_ssh_path() -> [ConnectionState; 7] {
        [
            ConnectionState::Validating,
            ConnectionState::ConnectingSsh,
            ConnectionState::VerifyingHostKey,
            ConnectionState::Authenticating,
            ConnectionState::OpeningForward,
            ConnectionState::CheckingService,
            ConnectionState::Connected,
        ]
    }

    #[test]
    fn ssh_happy_path_transitions_are_legal() {
        let mut state = ConnectionState::Idle;
        for next in happy_ssh_path() {
            state = state.transition(next).expect("legal transition");
        }
        assert_eq!(state, ConnectionState::Connected);
    }

    #[test]
    fn direct_path_can_skip_ssh_stages() {
        let state = ConnectionState::Idle;
        let state = state.transition(ConnectionState::Validating).unwrap();
        let state = state.transition(ConnectionState::CheckingService).unwrap();
        let final_state = state.transition(ConnectionState::Connected).unwrap();
        assert_eq!(final_state, ConnectionState::Connected);
    }

    #[test]
    fn any_active_state_can_disconnect() {
        for active in [
            ConnectionState::Validating,
            ConnectionState::ConnectingSsh,
            ConnectionState::Authenticating,
            ConnectionState::CheckingService,
            ConnectionState::Connected,
            ConnectionState::Reconnecting,
        ] {
            assert_eq!(
                active.transition(ConnectionState::Disconnecting).unwrap(),
                ConnectionState::Disconnecting
            );
        }
    }

    #[test]
    fn illegal_transition_is_rejected() {
        let res = ConnectionState::Idle.transition(ConnectionState::Connected);
        assert!(res.is_err());
        let res = ConnectionState::Connected.transition(ConnectionState::Authenticating);
        assert!(res.is_err());
    }

    #[test]
    fn reconnecting_can_reach_connected_or_error() {
        let r = ConnectionState::Reconnecting;
        assert_eq!(
            r.transition(ConnectionState::Connected).unwrap(),
            ConnectionState::Connected
        );
        assert_eq!(
            r.transition(ConnectionState::Error).unwrap(),
            ConnectionState::Error
        );
    }

    #[test]
    fn api_name_is_snake_case() {
        assert_eq!(
            ConnectionState::VerifyingHostKey.api_name(),
            "verifying_host_key"
        );
    }

    #[test]
    fn serde_round_trips_with_snake_case() {
        let s = serde_json::to_string(&ConnectionState::CheckingService).unwrap();
        assert_eq!(s, "\"checking_service\"");
        let back: ConnectionState = serde_json::from_str(&s).unwrap();
        assert_eq!(back, ConnectionState::CheckingService);
    }
}
