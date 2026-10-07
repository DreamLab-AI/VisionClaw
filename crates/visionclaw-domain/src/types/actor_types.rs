//! Actor-layer domain types.
//!
//! Domain representations of types used by ports and actors.
//! The actor layer wraps these with Actix Message derives.

use serde::{Deserialize, Serialize};

/// Physics simulation state — domain representation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicsState {
    pub is_running: bool,
    pub params: crate::models::simulation_params::SimulationParams,
}

/// Auto-balance notification — domain representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBalanceNotification {
    pub message: String,
    pub timestamp: i64,
    pub severity: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physics_state_default_is_not_running() {
        let ps = PhysicsState::default();
        assert!(!ps.is_running);
        // PhysicsSettings::default has enabled = true, so SimulationParams inherits that
        assert!(ps.params.enabled);
    }

    #[test]
    fn physics_state_serde_roundtrip() {
        let ps = PhysicsState::default();
        let json = serde_json::to_string(&ps).unwrap();
        let back: PhysicsState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.is_running, ps.is_running);
        assert_eq!(back.params.dt, ps.params.dt);
    }

    #[test]
    fn auto_balance_notification_fields() {
        let n = AutoBalanceNotification {
            message: "rebalanced".to_string(),
            timestamp: 1_700_000_000,
            severity: "info".to_string(),
        };
        let json = serde_json::to_string(&n).unwrap();
        let back: AutoBalanceNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message, "rebalanced");
        assert_eq!(back.timestamp, 1_700_000_000);
        assert_eq!(back.severity, "info");
    }
}
