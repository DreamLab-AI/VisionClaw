// Utility / helper functions for settings handling

use serde_json::{json, Value};

#[allow(dead_code)]
pub fn get_field_variant<'a>(obj: &'a Value, variants: &[&str]) -> Option<&'a Value> {
    for variant in variants {
        if let Some(val) = obj.get(*variant) {
            return Some(val);
        }
    }
    None
}

#[allow(dead_code)]
pub fn count_fields(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len() + map.values().map(count_fields).sum::<usize>(),
        Value::Array(arr) => arr.iter().map(count_fields).sum(),
        _ => 0,
    }
}

pub fn extract_physics_updates(update: &Value) -> Vec<&str> {
    update
        .get("visualisation")
        .and_then(|v| v.get("graphs"))
        .and_then(|g| g.as_object())
        .map(|graphs| {
            let mut updated = Vec::new();
            if graphs
                .get("knowledge")
                .and_then(|g| g.get("physics"))
                .is_some()
            {
                updated.push("knowledge");
            }
            if graphs.contains_key("visionclaw")
                && graphs
                    .get("visionclaw")
                    .and_then(|g| g.get("physics"))
                    .is_some()
            {
                updated.push("visionclaw");
            }
            updated
        })
        .unwrap_or_default()
}

#[allow(dead_code)]
pub fn extract_failed_field(physics: &Value) -> String {
    if let Some(obj) = physics.as_object() {
        obj.keys().next().unwrap_or(&"unknown".to_string()).clone()
    } else {
        "unknown".to_string()
    }
}

pub fn create_physics_settings_update(physics_update: Value) -> Value {
    let mut normalized_physics = physics_update.clone();

    if let Some(obj) = normalized_physics.as_object_mut() {
        if let Some(spring_strength) = obj.remove("springStrength") {
            if !obj.contains_key("springK") {
                obj.insert("springK".to_string(), spring_strength);
            }
        }

        if let Some(repulsion_strength) = obj.remove("repulsionStrength") {
            if !obj.contains_key("repelK") {
                obj.insert("repelK".to_string(), repulsion_strength);
            }
        }

        if let Some(attraction_strength) = obj.remove("attractionStrength") {
            if !obj.contains_key("attractionK") {
                obj.insert("attractionK".to_string(), attraction_strength);
            }
        }

        if let Some(collision_radius) = obj.remove("collisionRadius") {
            if !obj.contains_key("separationRadius") {
                obj.insert("separationRadius".to_string(), collision_radius);
            }
        }
    }

    json!({
        "visualisation": {
            "graphs": {
                "knowledge": {
                    "physics": normalized_physics
                },
                "visionclaw": {
                    "physics": normalized_physics.clone()
                }
            }
        }
    })
}
