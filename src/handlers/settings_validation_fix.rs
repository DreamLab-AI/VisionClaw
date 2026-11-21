// GPU constraint validation used by the constraints handler.
use serde_json::Value;

pub fn validate_constraint(constraint: &Value) -> Result<(), String> {
    if let Some(kind) = constraint.get("kind") {
        let val = kind
            .as_u64()
            .or_else(|| kind.as_i64().map(|i| i as u64))
            .ok_or("constraint kind must be an integer")?;
        if val > 10 {
            return Err("Invalid constraint kind".to_string());
        }
    }

    if let Some(nodes) = constraint.get("nodeIndices") {
        let indices = nodes.as_array().ok_or("nodeIndices must be an array")?;
        for (i, idx) in indices.iter().enumerate() {
            let val = idx
                .as_u64()
                .ok_or(format!("nodeIndices[{}] must be a positive integer", i))?;
            if val > 1_000_000 {
                return Err(format!("nodeIndices[{}] is too large", i));
            }
        }
    }

    if let Some(frame) = constraint.get("activationFrame") {
        let val = frame.as_i64().ok_or("activationFrame must be an integer")?;
        if val < 0 {
            return Err("activationFrame cannot be negative".to_string());
        }
    }

    if let Some(strength) = constraint.get("strength") {
        let val = strength.as_f64().ok_or("strength must be a number")?;
        if val < 0.0 || val > 10.0 {
            return Err("constraint strength must be between 0.0 and 10.0".to_string());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn test_constraint_validation() {
        let valid = json!({
            "kind": 1,
            "nodeIndices": [0, 5, 10],
            "activationFrame": 100,
            "strength": 2.5
        });
        assert!(validate_constraint(&valid).is_ok());

        let invalid_frame = json!({"activationFrame": -10});
        assert!(validate_constraint(&invalid_frame).is_err());
    }
}
