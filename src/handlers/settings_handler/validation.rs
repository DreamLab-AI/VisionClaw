// Validation logic for settings updates

use log::debug;
use serde_json::Value;

// Import comprehensive validation for GPU parameters
use crate::handlers::settings_validation_fix::validate_physics_settings_complete;

pub fn validate_settings_update(update: &Value) -> Result<(), String> {
    if let Some(vis) = update.get("visualisation") {
        if let Some(graphs) = vis.get("graphs") {
            for (graph_name, graph_settings) in
                graphs.as_object().ok_or("graphs must be an object")?.iter()
            {
                // ADR-2115: `logseq` is no longer a recognised graph name.
                let canonical = crate::config::normalise_graph_type(graph_name);
                if canonical != "knowledge" && canonical != "visionclaw" {
                    return Err(format!("Invalid graph name: {}", graph_name));
                }

                if let Some(physics) = graph_settings.get("physics") {
                    validate_physics_settings(physics)?;
                }

                if let Some(nodes) = graph_settings.get("nodes") {
                    validate_node_settings(nodes)?;
                }
            }
        }

        if let Some(rendering) = vis.get("rendering") {
            validate_rendering_settings(rendering)?;
        }

        if let Some(hologram) = vis.get("hologram") {
            validate_hologram_settings(hologram)?;
        }
    }

    if let Some(xr) = update.get("xr") {
        validate_xr_settings(xr)?;
    }

    if let Some(system) = update.get("system") {
        validate_system_settings(system)?;
    }

    Ok(())
}

fn validate_physics_settings(physics: &Value) -> Result<(), String> {
    validate_physics_settings_complete(physics)?;

    if let Some(obj) = physics.as_object() {
        debug!(
            "Physics settings fields received: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }

    if let Some(iterations) = physics.get("iterations") {
        let val = iterations
            .as_f64()
            .map(|f| f.round() as u64)
            .or_else(|| iterations.as_u64())
            .ok_or("iterations must be a positive number")?;
        if val == 0 || val > 1000 {
            return Err("iterations must be between 1 and 1000".to_string());
        }
    }

    if let Some(auto_balance_interval) = physics.get("autoBalanceIntervalMs") {
        let val = auto_balance_interval
            .as_u64()
            .or_else(|| auto_balance_interval.as_f64().map(|f| f.round() as u64))
            .ok_or("autoBalanceIntervalMs must be a positive integer")?;
        if val < 10 || val > 60000 {
            return Err("autoBalanceIntervalMs must be between 10 and 60000 ms".to_string());
        }
    }

    if let Some(boundary_limit) = physics.get("boundaryLimit") {
        let val = boundary_limit
            .as_f64()
            .ok_or("boundaryLimit must be a number")?;
        if val < 0.1 || val > 100000.0 {
            return Err("boundaryLimit must be between 0.1 and 100000.0".to_string());
        }

        if let Some(bounds_size) = physics.get("boundsSize").and_then(|b| b.as_f64()) {
            let max_boundary = bounds_size * 0.99;
            if val > max_boundary {
                return Err(format!(
                    "boundaryLimit ({:.1}) must be less than 99% of boundsSize ({:.1})",
                    val, bounds_size
                ));
            }
        }
    }

    Ok(())
}

fn validate_node_settings(nodes: &Value) -> Result<(), String> {
    if let Some(color) = nodes.get("baseColor") {
        let color_str = color.as_str().ok_or("baseColor must be a string")?;
        if !color_str.starts_with('#') || (color_str.len() != 7 && color_str.len() != 4) {
            return Err("baseColor must be a valid hex color (e.g., #ffffff or #fff)".to_string());
        }
    }

    if let Some(opacity) = nodes.get("opacity") {
        let val = opacity.as_f64().ok_or("opacity must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("opacity must be between 0.0 and 1.0".to_string());
        }
    }

    if let Some(metalness) = nodes.get("metalness") {
        let val = metalness.as_f64().ok_or("metalness must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("metalness must be between 0.0 and 1.0".to_string());
        }
    }

    if let Some(roughness) = nodes.get("roughness") {
        let val = roughness.as_f64().ok_or("roughness must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("roughness must be between 0.0 and 1.0".to_string());
        }
    }

    if let Some(node_size) = nodes.get("nodeSize") {
        let val = node_size.as_f64().ok_or("nodeSize must be a number")?;
        if val <= 0.0 || val > 1000.0 {
            return Err("nodeSize must be between 0.0 and 1000.0".to_string());
        }
    }

    if let Some(quality) = nodes.get("quality") {
        let q = quality.as_str().ok_or("quality must be a string")?;
        if !["low", "medium", "high"].contains(&q) {
            return Err("quality must be 'low', 'medium', or 'high'".to_string());
        }
    }

    Ok(())
}

fn validate_rendering_settings(rendering: &Value) -> Result<(), String> {
    if let Some(ambient) = rendering.get("ambientLightIntensity") {
        let val = ambient
            .as_f64()
            .ok_or("ambientLightIntensity must be a number")?;
        if val < 0.0 || val > 100.0 {
            return Err("ambientLightIntensity must be between 0.0 and 100.0".to_string());
        }
    }

    if let Some(glow) = rendering.get("glow") {
        validate_glow_settings(glow)?;
    }

    Ok(())
}

fn validate_glow_settings(glow: &Value) -> Result<(), String> {
    if let Some(enabled) = glow.get("enabled") {
        if !enabled.is_boolean() {
            return Err("glow enabled must be a boolean".to_string());
        }
    }

    for field_name in ["intensity", "strength"] {
        if let Some(intensity) = glow.get(field_name) {
            let val = intensity
                .as_f64()
                .ok_or(format!("glow {} must be a number", field_name))?;
            if val < 0.0 || val > 10.0 {
                return Err(format!("glow {} must be between 0.0 and 10.0", field_name));
            }
        }
    }

    if let Some(radius) = glow.get("radius") {
        let val = radius.as_f64().ok_or("glow radius must be a number")?;
        if val < 0.0 || val > 5.0 {
            return Err("glow radius must be between 0.0 and 5.0".to_string());
        }
    }

    if let Some(threshold) = glow.get("threshold") {
        let val = threshold
            .as_f64()
            .ok_or("glow threshold must be a number")?;
        if val < 0.0 || val > 2.0 {
            return Err("glow threshold must be between 0.0 and 2.0".to_string());
        }
    }

    for field_name in [
        "edgeGlowStrength",
        "environmentGlowStrength",
        "nodeGlowStrength",
    ] {
        if let Some(strength) = glow.get(field_name) {
            let val = strength
                .as_f64()
                .ok_or(format!("glow {} must be a number", field_name))?;
            if val < 0.0 || val > 1.0 {
                return Err(format!("glow {} must be between 0.0 and 1.0", field_name));
            }
        }
    }

    Ok(())
}

fn validate_hologram_settings(hologram: &Value) -> Result<(), String> {
    if let Some(ring_count) = hologram.get("ringCount") {
        let val = ring_count
            .as_f64()
            .map(|f| f.round() as u64)
            .or_else(|| ring_count.as_u64())
            .ok_or("ringCount must be a positive integer")?;

        if val > 20 {
            return Err("ringCount must be between 0 and 20".to_string());
        }
    }

    if let Some(color) = hologram.get("ringColor") {
        let color_str = color.as_str().ok_or("ringColor must be a string")?;
        if !color_str.starts_with('#') || (color_str.len() != 7 && color_str.len() != 4) {
            return Err("ringColor must be a valid hex color (e.g., #ffffff or #fff)".to_string());
        }
    }

    if let Some(opacity) = hologram.get("ringOpacity") {
        let val = opacity.as_f64().ok_or("ringOpacity must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("ringOpacity must be between 0.0 and 1.0".to_string());
        }
    }

    if let Some(speed) = hologram.get("ringRotationSpeed") {
        let val = speed.as_f64().ok_or("ringRotationSpeed must be a number")?;
        if val < 0.0 || val > 1000.0 {
            return Err("ringRotationSpeed must be between 0.0 and 1000.0".to_string());
        }
    }

    Ok(())
}

fn validate_system_settings(system: &Value) -> Result<(), String> {
    if let Some(debug) = system.get("debug") {
        if let Some(debug_obj) = debug.as_object() {
            let boolean_fields = [
                "enabled",
                "showFPS",
                "showMemory",
                "enablePerformanceDebug",
                "enableTelemetry",
                "enableDataDebug",
                "enableWebSocketDebug",
                "enablePhysicsDebug",
                "enableNodeDebug",
                "enableShaderDebug",
                "enableMatrixDebug",
            ];

            for field in &boolean_fields {
                if let Some(val) = debug_obj.get(*field) {
                    if !val.is_boolean() {
                        return Err(format!("debug.{} must be a boolean", field));
                    }
                }
            }

            if let Some(log_level) = debug_obj.get("logLevel") {
                if let Some(val) = log_level.as_f64() {
                    if val < 0.0 || val > 3.0 {
                        return Err("debug.logLevel must be between 0 and 3".to_string());
                    }
                } else if let Some(val) = log_level.as_u64() {
                    if val > 3 {
                        return Err("debug.logLevel must be between 0 and 3".to_string());
                    }
                } else if let Some(val) = log_level.as_str() {
                    match val {
                        "error" | "warn" | "info" | "debug" => {}
                        _ => {
                            return Err(
                                "debug.logLevel must be 'error', 'warn', 'info', or 'debug'"
                                    .to_string(),
                            );
                        }
                    }
                } else {
                    return Err("debug.logLevel must be a number or string".to_string());
                }
            }
        }
    }

    if let Some(persist) = system.get("persistSettingsOnServer") {
        if !persist.is_boolean() {
            return Err("system.persistSettingsOnServer must be a boolean".to_string());
        }
    }

    if let Some(url) = system.get("customBackendUrl") {
        if !url.is_string() && !url.is_null() {
            return Err("system.customBackendUrl must be a string or null".to_string());
        }
    }

    Ok(())
}

pub fn validate_xr_settings(xr: &Value) -> Result<(), String> {
    if let Some(enabled) = xr.get("enabled") {
        if !enabled.is_boolean() {
            return Err("XR enabled must be a boolean".to_string());
        }
    }

    if let Some(quality) = xr.get("quality") {
        if let Some(q) = quality.as_str() {
            if !["Low", "Medium", "High", "low", "medium", "high"].contains(&q) {
                return Err("XR quality must be Low, Medium, or High".to_string());
            }
        } else {
            return Err("XR quality must be a string".to_string());
        }
    }

    if let Some(render_scale) = xr.get("renderScale") {
        let val = render_scale
            .as_f64()
            .ok_or("renderScale must be a number")?;
        if val < 0.1 || val > 10.0 {
            return Err("renderScale must be between 0.1 and 10.0".to_string());
        }
    }

    if let Some(room_scale) = xr.get("roomScale") {
        let val = room_scale.as_f64().ok_or("roomScale must be a number")?;
        if val <= 0.0 || val > 100.0 {
            return Err("roomScale must be between 0.0 and 100.0".to_string());
        }
    }

    if let Some(hand_tracking) = xr.get("handTracking") {
        if let Some(ht_obj) = hand_tracking.as_object() {
            if let Some(enabled) = ht_obj.get("enabled") {
                if !enabled.is_boolean() {
                    return Err("handTracking.enabled must be a boolean".to_string());
                }
            }
        }
    }

    if let Some(interactions) = xr.get("interactions") {
        if let Some(int_obj) = interactions.as_object() {
            if let Some(haptics) = int_obj.get("enableHaptics") {
                if !haptics.is_boolean() {
                    return Err("interactions.enableHaptics must be a boolean".to_string());
                }
            }
        }
    }

    Ok(())
}

pub fn validate_constraints(constraints: &Value) -> Result<(), String> {
    if let Some(obj) = constraints.as_object() {
        for (constraint_type, constraint_data) in obj {
            if !["separation", "boundary", "alignment", "cluster"]
                .contains(&constraint_type.as_str())
            {
                return Err(format!("Unknown constraint type: {}", constraint_type));
            }

            if let Some(data) = constraint_data.as_object() {
                if let Some(strength) = data.get("strength") {
                    let val = strength.as_f64().ok_or("strength must be a number")?;
                    if val < 0.0 || val > 100.0 {
                        return Err("strength must be between 0.0 and 100.0".to_string());
                    }
                }

                if let Some(enabled) = data.get("enabled") {
                    if !enabled.is_boolean() {
                        return Err("enabled must be a boolean".to_string());
                    }
                }
            }
        }
    }

    Ok(())
}
