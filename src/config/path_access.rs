use serde_json::Value;
use std::any::Any;

pub trait PathAccessible {
    
    fn get_by_path(&self, path: &str) -> Result<Box<dyn Any>, String>;

    
    fn set_by_path(&mut self, path: &str, value: Box<dyn Any>) -> Result<(), String>;
}

pub trait JsonPathAccessible: serde::Serialize + serde::de::DeserializeOwned {
    
    fn get_json_by_path(&self, path: &str) -> Result<Value, String> {
        
        let root = serde_json::to_value(self).map_err(|e| format!("Failed to serialize: {}", e))?;

        
        navigate_json_path(&root, path).ok_or_else(|| format!("Path '{}' not found", path))
    }

    
    fn set_json_by_path(&mut self, path: &str, value: Value) -> Result<(), String> {
        
        let mut root =
            serde_json::to_value(&*self).map_err(|e| format!("Failed to serialize: {}", e))?;

        
        set_json_at_path(&mut root, path, value)?;

        
        *self =
            serde_json::from_value(root).map_err(|e| format!("Failed to deserialize: {}", e))?;

        Ok(())
    }
}

// Implement JsonPathAccessible for all types that have Serialize + DeserializeOwned
impl<T: serde::Serialize + serde::de::DeserializeOwned> JsonPathAccessible for T {}

pub fn parse_path(path: &str) -> Result<Vec<&str>, String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    let segments: Vec<&str> = path.split('.').collect();

    
    if segments.iter().any(|s| s.is_empty()) {
        return Err("Path segments cannot be empty".to_string());
    }

    Ok(segments)
}

#[allow(unused_macros)]
macro_rules! impl_field_access {
    ($struct_name:ident, {
        $($field:ident => $field_type:ty),*
    }) => {
        impl PathAccessible for $struct_name {
            fn get_by_path(&self, path: &str) -> Result<Box<dyn Any>, String> {
                let segments = parse_path(path)?;

                match segments[0] {
                    $(
                        stringify!($field) => {
                            if segments.len() == 1 {
                                Ok(Box::new(self.$field.clone()))
                            } else {
                                
                                let remaining = segments[1..].join(".");
                                self.$field.get_by_path(&remaining)
                            }
                        }
                    )*
                    _ => Err(format!("Unknown field: {}", segments[0]))
                }
            }

            fn set_by_path(&mut self, path: &str, value: Box<dyn Any>) -> Result<(), String> {
                let segments = parse_path(path)?;

                match segments[0] {
                    $(
                        stringify!($field) => {
                            if segments.len() == 1 {
                                match value.downcast::<$field_type>() {
                                    Ok(v) => {
                                        self.$field = *v;
                                        Ok(())
                                    }
                                    Err(_) => Err(format!("Type mismatch for field {}", segments[0]))
                                }
                            } else {
                                
                                let remaining = segments[1..].join(".");
                                self.$field.set_by_path(&remaining, value)
                            }
                        }
                    )*
                    _ => Err(format!("Unknown field: {}", segments[0]))
                }
            }
        }
    };
}

// Make the macro available to other modules
#[allow(unused_imports)]
pub(crate) use impl_field_access;

fn navigate_json_path(root: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return Some(root.clone());
    }

    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    let mut current = root;

    for segment in segments {
        match current {
            Value::Object(map) => {
                
                current = map
                    .get(segment)
                    .or_else(|| map.get(&camel_to_snake_case(segment)))
                    .or_else(|| map.get(&snake_to_camel_case(segment)))?;
            }
            _ => return None,
        }
    }

    Some(current.clone())
}

fn set_json_at_path(root: &mut Value, path: &str, value: Value) -> Result<(), String> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }

    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();

    if segments.is_empty() {
        return Err("Invalid empty path".to_string());
    }

    
    if !validate_path_exists(root, &segments) {
        return Err(format!(
            "Path '{}' does not exist in the settings structure",
            path
        ));
    }

    
    let mut current = root;

    for (i, segment) in segments.iter().enumerate() {
        if i == segments.len() - 1 {
            
            match current {
                Value::Object(map) => {
                    
                    
                    let field_key = find_field_key(map, segment)
                        .ok_or_else(|| format!("Field '{}' not found in object", segment))?;

                    
                    if let Some(existing) = map.get(&field_key) {
                        if !values_have_compatible_types(existing, &value) {
                            return Err(format!(
                                "Type mismatch for field '{}': expected {}, got {}",
                                segment,
                                value_type_name(existing),
                                value_type_name(&value)
                            ));
                        }
                    }

                    
                    let final_value = if let Some(existing) = map.get(&field_key) {
                        match (existing, &value) {
                            (Value::Number(_), Value::String(s)) => {
                                
                                if let Ok(num) = s.parse::<f64>() {
                                    serde_json::Number::from_f64(num)
                                        .map(Value::Number)
                                        .unwrap_or(value)
                                } else {
                                    value
                                }
                            }
                            _ => value,
                        }
                    } else {
                        value
                    };

                    map.insert(field_key, final_value);
                    return Ok(());
                }
                _ => return Err(format!("Parent of '{}' is not an object", segment)),
            }
        } else {
            
            match current {
                Value::Object(map) => {
                    
                    let field_key = find_field_key(map, segment).ok_or_else(|| {
                        format!("Field '{}' not found while navigating path", segment)
                    })?;

                    current = map.get_mut(&field_key).ok_or_else(|| {
                        format!("Failed to get mutable reference to field '{}'", segment)
                    })?;
                }
                _ => {
                    return Err(format!(
                        "Cannot navigate through non-object at '{}'",
                        segment
                    ))
                }
            }
        }
    }

    Ok(())
}

fn find_field_key(map: &serde_json::Map<String, Value>, segment: &str) -> Option<String> {
    
    if map.contains_key(segment) {
        return Some(segment.to_string());
    }

    
    let camel_case = snake_to_camel_case(segment);
    if map.contains_key(&camel_case) {
        return Some(camel_case);
    }

    
    let snake_case = camel_to_snake_case(segment);
    if map.contains_key(&snake_case) {
        return Some(snake_case);
    }

    None
}

fn snake_to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

fn camel_to_snake_case(s: &str) -> String {
    let mut result = String::new();

    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_ascii_lowercase());
    }

    result
}

fn validate_path_exists(root: &Value, segments: &[&str]) -> bool {
    let mut current = root;

    for segment in segments {
        match current {
            Value::Object(map) => {
                
                if let Some(field_key) = find_field_key(map, segment) {
                    current = &map[&field_key];
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }

    true
}

fn values_have_compatible_types(existing: &Value, new_value: &Value) -> bool {
    match (existing, new_value) {
        (Value::Null, _) | (_, Value::Null) => true,
        (Value::Bool(_), Value::Bool(_)) => true,
        (Value::Number(_), Value::Number(_)) => true,
        
        (Value::Number(_), Value::String(s)) => s.parse::<f64>().is_ok(),
        
        (Value::String(_), Value::Number(_)) => true,
        (Value::String(_), Value::String(_)) => true,
        (Value::Array(_), Value::Array(_)) => true,
        (Value::Object(_), Value::Object(_)) => true,
        _ => false,
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_parse_path() {
        assert_eq!(parse_path("a.b.c").unwrap(), vec!["a", "b", "c"]);
        assert_eq!(parse_path("single").unwrap(), vec!["single"]);
        assert!(parse_path("").is_err());
        assert!(parse_path("a..b").is_err());
    }

    #[test]
    fn test_case_conversion() {
        assert_eq!(snake_to_camel_case("max_velocity"), "maxVelocity");
        assert_eq!(snake_to_camel_case("enable_hologram"), "enableHologram");
        assert_eq!(camel_to_snake_case("maxVelocity"), "max_velocity");
        assert_eq!(camel_to_snake_case("enableHologram"), "enable_hologram");
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")] 
    struct TestSettings {
        enable_hologram: bool, 
        max_velocity: f32,     
        auto_balance: bool,    
    }

    #[test]
    fn test_json_path_get() {
        let settings = TestSettings {
            enable_hologram: true,
            max_velocity: 10.0,
            auto_balance: false,
        };

        
        let value = settings.get_json_by_path("enableHologram").unwrap();
        assert_eq!(value, Value::Bool(true));

        let value = settings.get_json_by_path("maxVelocity").unwrap();
        assert_eq!(
            value,
            Value::Number(safe_json_number(10.0))
        );
    }

    #[test]
    fn test_json_path_set() {
        let mut settings = TestSettings {
            enable_hologram: true,
            max_velocity: 10.0,
            auto_balance: false,
        };

        
        settings
            .set_json_by_path("enableHologram", Value::Bool(false))
            .unwrap();
        assert_eq!(settings.enable_hologram, false);

        settings
            .set_json_by_path("autoBalance", Value::Bool(true))
            .unwrap();
        assert_eq!(settings.auto_balance, true);

        settings
            .set_json_by_path(
                "maxVelocity",
                Value::Number(safe_json_number(25.5)),
            )
            .unwrap();
        assert_eq!(settings.max_velocity, 25.5);
    }

    #[test]
    fn test_nested_json_path() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct NestedSettings {
            visualisation: VisualisationPart,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct VisualisationPart {
            enable_hologram: bool,
            physics: PhysicsPart,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct PhysicsPart {
            max_velocity: f32,
        }

        let mut settings = NestedSettings {
            visualisation: VisualisationPart {
                enable_hologram: true,
                physics: PhysicsPart {
                    max_velocity: 100.0,
                },
            },
        };

        
        let value = settings
            .get_json_by_path("visualisation.enableHologram")
            .unwrap();
        assert_eq!(value, Value::Bool(true));

        let value = settings
            .get_json_by_path("visualisation.physics.maxVelocity")
            .unwrap();
        assert_eq!(
            value,
            Value::Number(safe_json_number(100.0))
        );

        
        settings
            .set_json_by_path(
                "visualisation.physics.maxVelocity",
                Value::Number(safe_json_number(200.0)),
            )
            .unwrap();
        assert_eq!(settings.visualisation.physics.max_velocity, 200.0);
    }

    #[test]
    fn test_path_validation() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct TestSettings {
            enable_feature: bool,
            max_count: u32,
        }

        let mut settings = TestSettings {
            enable_feature: true,
            max_count: 10,
        };

        
        assert!(settings
            .set_json_by_path("enableFeature", Value::Bool(false))
            .is_ok());
        assert_eq!(settings.enable_feature, false);

        
        let result = settings.set_json_by_path("nonExistentField", Value::Bool(true));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));

        
        let result = settings.set_json_by_path("maxCount", Value::Bool(true));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Type mismatch"));
    }

    #[test]
    fn test_batch_update_scenario() {
        
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct Settings {
            visualisation: Vis,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct Vis {
            enable_hologram: bool,
            hologram_settings: HologramSettings,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct HologramSettings {
            ring_count: u32,
        }

        let mut settings = Settings {
            visualisation: Vis {
                enable_hologram: false,
                hologram_settings: HologramSettings { ring_count: 3 },
            },
        };

        
        let updates = vec![
            ("visualisation.enableHologram", Value::Bool(true)),
            (
                "visualisation.hologramSettings.ringCount",
                Value::Number(5.into()),
            ),
        ];

        for (path, value) in updates {
            settings.set_json_by_path(path, value).unwrap();
        }

        assert_eq!(settings.visualisation.enable_hologram, true);
        assert_eq!(settings.visualisation.hologram_settings.ring_count, 5);

        
        let json = serde_json::to_value(&settings).unwrap();
        let deserialized: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(settings, deserialized);
    }
}
