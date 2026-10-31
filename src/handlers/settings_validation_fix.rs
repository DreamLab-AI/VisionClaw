// Complete validation fixes for all GPU parameters and case conversion issues
use serde_json::Value;
use std::collections::HashMap;

pub fn validate_physics_settings_complete(physics: &Value) -> Result<(), String> {
    if let Some(dt) = physics.get("dt").or_else(|| physics.get("timeStep")) {
        let val = dt.as_f64().ok_or("dt must be a number")?;
        if val <= 0.0 || val > 0.1 {
            return Err("dt must be between 0.001 and 0.1 for GPU stability".to_string());
        }
    }

    if let Some(max_vel) = physics.get("maxVelocity") {
        let val = max_vel.as_f64().ok_or("maxVelocity must be a number")?;
        if val <= 0.0 || val > 100.0 {
            return Err("maxVelocity must be between 0.1 and 100.0".to_string());
        }
    }

    if let Some(repel_k) = physics.get("repelK") {
        let val = repel_k.as_f64().ok_or("repelK must be a number")?;
        if val <= 0.0 || val > 500.0 {
            return Err("repelK must be between 0.001 and 500.0".to_string());
        }
    }

    if let Some(spring_k) = physics.get("springK") {
        let val = spring_k.as_f64().ok_or("springK must be a number")?;
        if val <= 0.0 || val > 10.0 {
            return Err("springK must be between 0.001 and 10.0".to_string());
        }
    }

    if let Some(damping) = physics.get("damping") {
        let val = damping.as_f64().ok_or("damping must be a number")?;
        if val < 0.0 || val >= 1.0 {
            return Err("damping must be between 0.0 and 0.999".to_string());
        }
    }

    if let Some(max_force) = physics.get("maxForce") {
        let val = max_force.as_f64().ok_or("maxForce must be a number")?;
        if val <= 0.0 || val > 1000.0 {
            return Err("maxForce must be between 0.1 and 1000.0".to_string());
        }
    }

    if let Some(sssp_alpha) = physics.get("ssspAlpha") {
        let val = sssp_alpha.as_f64().ok_or("ssspAlpha must be a number")?;
        if val < 0.0 || val > 5.0 {
            return Err("ssspAlpha must be between 0.0 and 5.0".to_string());
        }
    }

    if let Some(constraint_strength) = physics.get("constraintStrength") {
        let val = constraint_strength
            .as_f64()
            .ok_or("constraintStrength must be a number")?;
        if val < 0.0 || val > 10.0 {
            return Err("constraintStrength must be between 0.0 and 10.0".to_string());
        }
    }

    Ok(())
}

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

pub fn convert_to_snake_case_recursive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (key, mut val) in map.clone() {
                let snake_key = camel_to_snake_case(&key);
                convert_to_snake_case_recursive(&mut val);
                new_map.insert(snake_key, val);
            }
            *map = new_map;
        }
        Value::Array(arr) => {
            for item in arr {
                convert_to_snake_case_recursive(item);
            }
        }
        _ => {}
    }
}

fn camel_to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_upper = false;

    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 && !prev_upper {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
            prev_upper = true;
        } else {
            result.push(ch);
            prev_upper = false;
        }
    }

    match result.as_str() {
        "s_s_s_p_alpha" => "sssp_alpha".to_string(),
        "s_s_s_p_enabled" => "sssp_enabled".to_string(),
        _ => result,
    }
}

pub fn get_complete_field_mappings() -> HashMap<String, String> {
    let mut mappings = HashMap::new();

    mappings.insert("springK".to_string(), "spring_k".to_string());
    mappings.insert("repelK".to_string(), "repel_k".to_string());
    mappings.insert("attractionK".to_string(), "spring_k".to_string());
    mappings.insert("centerGravityK".to_string(), "center_gravity_k".to_string());
    mappings.insert("maxVelocity".to_string(), "max_velocity".to_string());
    mappings.insert("maxForce".to_string(), "max_force".to_string());
    mappings.insert("timeStep".to_string(), "dt".to_string());
    mappings.insert("boundsSize".to_string(), "bounds_size".to_string());
    mappings.insert(
        "boundaryDamping".to_string(),
        "boundary_damping".to_string(),
    );
    mappings.insert(
        "separationRadius".to_string(),
        "separation_radius".to_string(),
    );
    mappings.insert(
        "alignmentStrength".to_string(),
        "alignment_strength".to_string(),
    );
    mappings.insert(
        "clusterStrength".to_string(),
        "cluster_strength".to_string(),
    );
    mappings.insert("computeMode".to_string(), "compute_mode".to_string());
    mappings.insert("minDistance".to_string(), "min_distance".to_string());
    mappings.insert(
        "maxRepulsionDist".to_string(),
        "max_repulsion_dist".to_string(),
    );
    mappings.insert("ssspAlpha".to_string(), "sssp_alpha".to_string());
    mappings.insert("ssspEnabled".to_string(), "sssp_enabled".to_string());
    mappings.insert("layoutMode".to_string(), "layout_mode".to_string());
    mappings.insert(
        "ssspSourceNodes".to_string(),
        "sssp_source_nodes".to_string(),
    );
    mappings.insert(
        "constraintStrength".to_string(),
        "constraint_strength".to_string(),
    );

    mappings.insert("enableHologram".to_string(), "enable_hologram".to_string());
    mappings.insert("showLabels".to_string(), "show_labels".to_string());
    mappings.insert("nodeSize".to_string(), "node_size".to_string());
    mappings.insert("edgeWidth".to_string(), "edge_width".to_string());
    mappings.insert("labelSize".to_string(), "label_size".to_string());
    mappings.insert(
        "showMetadataShape".to_string(),
        "show_metadata_shape".to_string(),
    );
    mappings.insert(
        "enableMetadataVisualisation".to_string(),
        "enable_metadata_visualisation".to_string(),
    );
    mappings.insert(
        "enableNodeAnimations".to_string(),
        "enable_node_animations".to_string(),
    );
    mappings.insert(
        "enableMotionBlur".to_string(),
        "enable_motion_blur".to_string(),
    );
    mappings.insert(
        "motionBlurStrength".to_string(),
        "motion_blur_strength".to_string(),
    );
    mappings.insert(
        "selectionWaveEnabled".to_string(),
        "selection_wave_enabled".to_string(),
    );
    mappings.insert("pulseEnabled".to_string(), "pulse_enabled".to_string());
    mappings.insert("pulseSpeed".to_string(), "pulse_speed".to_string());
    mappings.insert("pulseStrength".to_string(), "pulse_strength".to_string());
    mappings.insert("waveSpeed".to_string(), "wave_speed".to_string());
    mappings.insert(
        "ambientLightIntensity".to_string(),
        "ambient_light_intensity".to_string(),
    );
    mappings.insert(
        "backgroundColor".to_string(),
        "background_color".to_string(),
    );
    mappings.insert(
        "directionalLightIntensity".to_string(),
        "directional_light_intensity".to_string(),
    );
    mappings.insert(
        "enableAmbientOcclusion".to_string(),
        "enable_ambient_occlusion".to_string(),
    );
    mappings.insert(
        "enableAntialiasing".to_string(),
        "enable_antialiasing".to_string(),
    );
    mappings.insert("enableShadows".to_string(), "enable_shadows".to_string());
    mappings.insert(
        "environmentIntensity".to_string(),
        "environment_intensity".to_string(),
    );
    mappings.insert("shadowMapSize".to_string(), "shadow_map_size".to_string());
    mappings.insert("shadowBias".to_string(), "shadow_bias".to_string());
    mappings.insert("agentColors".to_string(), "agent_colors".to_string());

    mappings.insert("autoBalance".to_string(), "auto_balance".to_string());
    mappings.insert(
        "autoBalanceIntervalMs".to_string(),
        "auto_balance_interval_ms".to_string(),
    );
    mappings.insert(
        "autoBalanceConfig".to_string(),
        "auto_balance_config".to_string(),
    );

    mappings
}

pub fn apply_field_mappings(value: &mut Value, mappings: &HashMap<String, String>) {
    if let Value::Object(map) = value {
        let mut updates = Vec::new();

        for (key, val) in map.iter() {
            if let Some(new_key) = mappings.get(key) {
                if new_key != key {
                    updates.push((key.clone(), new_key.clone(), val.clone()));
                }
            }
        }

        for (old_key, new_key, val) in updates {
            map.remove(&old_key);
            map.insert(new_key, val);
        }

        for (_, val) in map.iter_mut() {
            apply_field_mappings(val, mappings);
        }
    } else if let Value::Array(arr) = value {
        for item in arr {
            apply_field_mappings(item, mappings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_camel_to_snake_case() {
        assert_eq!(camel_to_snake_case("springK"), "spring_k");
        assert_eq!(camel_to_snake_case("maxVelocity"), "max_velocity");
        assert_eq!(camel_to_snake_case("ssspAlpha"), "sssp_alpha");
        assert_eq!(camel_to_snake_case("enableHologram"), "enable_hologram");
    }

    #[test]
    fn test_physics_validation_bounds() {
        let valid = json!({
            "dt": 0.05,
            "maxVelocity": 50.0,
            "repelK": 100.0,
            "springK": 1.0,
            "damping": 0.5
        });
        assert!(validate_physics_settings_complete(&valid).is_ok());

        let invalid_dt = json!({"dt": 1.0});
        assert!(validate_physics_settings_complete(&invalid_dt).is_err());

        let invalid_repel = json!({"repelK": -10.0});
        assert!(validate_physics_settings_complete(&invalid_repel).is_err());
    }

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
