//! Per-node repulsion charge from a node's source domain.
//!
//! The force kernel multiplies a pair's base repulsion by
//! `charge[i] * charge[j]`, so a lower charge for classified nodes lets edges
//! pull same-domain nodes into clusters, and a higher charge pushes
//! unclassified nodes outwards.
//!
//! Two copies of this table used to exist with different values: 2.5 and 0.3
//! in `GPUResourceActor`, 1.2 and 0.6 in `ForceComputeActor` (estate register
//! ER-25). Only the force actor's upload runs on the live server, so its
//! values are the ones every layout to date was computed with, and they are
//! the ones kept here.

/// Charge for a node whose `vault_core::domains::domain_class_id` is `class_id`
/// (0 means no recognised domain).
pub fn domain_class_charge(class_id: i32) -> f32 {
    if class_id == 0 {
        UNCLASSIFIED_CHARGE
    } else {
        CLASSIFIED_CHARGE
    }
}

/// Charge for a node with no recognised domain.
pub const UNCLASSIFIED_CHARGE: f32 = 1.2;
/// Charge for a node in a recognised domain.
pub const CLASSIFIED_CHARGE: f32 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclassified_nodes_repel_more_than_classified_ones() {
        assert_eq!(domain_class_charge(0), UNCLASSIFIED_CHARGE);
        assert_eq!(domain_class_charge(3), CLASSIFIED_CHARGE);
        assert!(domain_class_charge(0) > domain_class_charge(1));
    }
}
