use super::path_access::{parse_path, PathAccessible};
use super::{AppFullSettings, AuthSettings, PhysicsSettings, SystemSettings};
use super::{GraphSettings, GraphsSettings, VisualisationSettings, XRSettings};

// PathAccessible implementation for AppFullSettings
impl PathAccessible for AppFullSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "visualisation" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.visualisation.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.visualisation.get_by_path(&remaining)
                }
            }
            "system" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.system.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.system.get_by_path(&remaining)
                }
            }
            "xr" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.xr.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.xr.get_by_path(&remaining)
                }
            }
            "auth" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.auth.clone()))
                } else {
                    Err("Auth fields are not deeply accessible".to_string())
                }
            }
            _ => Err(format!("Unknown top-level field: {}", segments[0])),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "visualisation" => {
                if segments.len() == 1 {
                    match value.downcast::<VisualisationSettings>() {
                        Ok(v) => {
                            self.visualisation = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for visualisation field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.visualisation.set_by_path(&remaining, value)
                }
            }
            "system" => {
                if segments.len() == 1 {
                    match value.downcast::<SystemSettings>() {
                        Ok(v) => {
                            self.system = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for system field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.system.set_by_path(&remaining, value)
                }
            }
            "xr" => {
                if segments.len() == 1 {
                    match value.downcast::<XRSettings>() {
                        Ok(v) => {
                            self.xr = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for xr field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.xr.set_by_path(&remaining, value)
                }
            }
            "auth" => {
                if segments.len() == 1 {
                    match value.downcast::<AuthSettings>() {
                        Ok(v) => {
                            self.auth = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for auth field".to_string()),
                    }
                } else {
                    Err("Auth nested fields are not modifiable".to_string())
                }
            }
            _ => Err(format!("Unknown top-level field: {}", segments[0])),
        }
    }
}

// Basic PathAccessible implementations for nested structures
impl PathAccessible for VisualisationSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "graphs" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.graphs.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.graphs.get_by_path(&remaining)
                }
            }
            _ => Err(format!(
                "Only graphs field is currently supported: {}",
                segments[0]
            )),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "graphs" => {
                if segments.len() == 1 {
                    match value.downcast::<GraphsSettings>() {
                        Ok(v) => {
                            self.graphs = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for graphs field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.graphs.set_by_path(&remaining, value)
                }
            }
            _ => Err("Only graphs field is currently supported for modification".to_string()),
        }
    }
}

impl PathAccessible for GraphsSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "knowledge" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.knowledge.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.knowledge.get_by_path(&remaining)
                }
            }
            "visionclaw" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.visionclaw.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.visionclaw.get_by_path(&remaining)
                }
            }
            _ => Err(format!("Unknown graph type: {}", segments[0])),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "knowledge" => {
                if segments.len() == 1 {
                    match value.downcast::<GraphSettings>() {
                        Ok(v) => {
                            self.knowledge = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for knowledge field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.knowledge.set_by_path(&remaining, value)
                }
            }
            "visionclaw" => {
                if segments.len() == 1 {
                    match value.downcast::<GraphSettings>() {
                        Ok(v) => {
                            self.visionclaw = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for visionclaw field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.visionclaw.set_by_path(&remaining, value)
                }
            }
            _ => Err(format!("Unknown graph type: {}", segments[0])),
        }
    }
}

impl PathAccessible for GraphSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "physics" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.physics.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.physics.get_by_path(&remaining)
                }
            }
            _ => Err(format!(
                "Only physics is supported currently: {}",
                segments[0]
            )),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "physics" => {
                if segments.len() == 1 {
                    match value.downcast::<PhysicsSettings>() {
                        Ok(v) => {
                            self.physics = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for physics field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.physics.set_by_path(&remaining, value)
                }
            }
            _ => Err("Only physics field is currently supported for modification".to_string()),
        }
    }
}

// Critical: PhysicsSettings PathAccessible implementation for performance fix
impl PathAccessible for PhysicsSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "damping" => Ok(Box::new(self.damping)),
            "springK" => Ok(Box::new(self.spring_k)),
            "repelK" => Ok(Box::new(self.repel_k)),
            "enabled" => Ok(Box::new(self.enabled)),
            "iterations" => Ok(Box::new(self.iterations)),
            "maxVelocity" => Ok(Box::new(self.max_velocity)),
            "boundsSize" => Ok(Box::new(self.bounds_size)),
            "gravity" => Ok(Box::new(self.gravity)),
            "temperature" => Ok(Box::new(self.temperature)),
            _ => Err(format!("Unknown physics field: {}", segments[0])),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "damping" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.damping = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for damping field".to_string()),
            },
            "springK" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.spring_k = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for springK field".to_string()),
            },
            "repelK" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.repel_k = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for repelK field".to_string()),
            },
            "enabled" => match value.downcast::<bool>() {
                Ok(v) => {
                    self.enabled = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for enabled field".to_string()),
            },
            "iterations" => match value.downcast::<u32>() {
                Ok(v) => {
                    self.iterations = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for iterations field".to_string()),
            },
            "maxVelocity" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.max_velocity = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for maxVelocity field".to_string()),
            },
            "boundsSize" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.bounds_size = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for boundsSize field".to_string()),
            },
            "gravity" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.gravity = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for gravity field".to_string()),
            },
            "temperature" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.temperature = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for temperature field".to_string()),
            },
            _ => Err(format!("Unknown physics field: {}", segments[0])),
        }
    }
}

// Implementation for SystemSettings path access
impl PathAccessible for SystemSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        match path {
            "network" => Ok(Box::new(self.network.clone())),
            "websocket" => Ok(Box::new(self.websocket.clone())),
            "security" => Ok(Box::new(self.security.clone())),
            "debug" => Ok(Box::new(self.debug.clone())),
            "persist_settings" => Ok(Box::new(self.persist_settings)),
            "custom_backend_url" => Ok(Box::new(self.custom_backend_url.clone())),
            _ => Err(format!("Unknown SystemSettings field: {}", path)),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        match path {
            "persist_settings" => {
                if let Some(val) = value.downcast_ref::<bool>() {
                    self.persist_settings = *val;
                    Ok(())
                } else {
                    Err("Invalid type for persist_settings, expected bool".to_string())
                }
            }
            "custom_backend_url" => {
                if let Some(val) = value.downcast_ref::<Option<String>>() {
                    self.custom_backend_url = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for custom_backend_url, expected Option<String>".to_string())
                }
            }
            _ => Err(format!("Setting {} not supported for SystemSettings", path)),
        }
    }
}

impl PathAccessible for XRSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        match path {
            "enabled" => Ok(Box::new(self.enabled.clone())),
            "client_side_enable_xr" => Ok(Box::new(self.client_side_enable_xr.clone())),
            "mode" => Ok(Box::new(self.mode.clone())),
            "room_scale" => Ok(Box::new(self.room_scale)),
            "space_type" => Ok(Box::new(self.space_type.clone())),
            "quality" => Ok(Box::new(self.quality.clone())),
            "render_scale" => Ok(Box::new(self.render_scale.clone())),
            "interaction_distance" => Ok(Box::new(self.interaction_distance)),
            "locomotion_method" => Ok(Box::new(self.locomotion_method.clone())),
            "teleport_ray_color" => Ok(Box::new(self.teleport_ray_color.clone())),
            "controller_ray_color" => Ok(Box::new(self.controller_ray_color.clone())),
            _ => Err(format!("Unknown XRSettings field: {}", path)),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        match path {
            "enabled" => {
                if let Some(val) = value.downcast_ref::<Option<bool>>() {
                    self.enabled = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for enabled, expected Option<bool>".to_string())
                }
            }
            "room_scale" => {
                if let Some(val) = value.downcast_ref::<f32>() {
                    self.room_scale = *val;
                    Ok(())
                } else {
                    Err("Invalid type for room_scale, expected f32".to_string())
                }
            }
            "space_type" => {
                if let Some(val) = value.downcast_ref::<String>() {
                    self.space_type = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for space_type, expected String".to_string())
                }
            }
            "quality" => {
                if let Some(val) = value.downcast_ref::<String>() {
                    self.quality = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for quality, expected String".to_string())
                }
            }
            "interaction_distance" => {
                if let Some(val) = value.downcast_ref::<f32>() {
                    self.interaction_distance = *val;
                    Ok(())
                } else {
                    Err("Invalid type for interaction_distance, expected f32".to_string())
                }
            }
            "locomotion_method" => {
                if let Some(val) = value.downcast_ref::<String>() {
                    self.locomotion_method = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for locomotion_method, expected String".to_string())
                }
            }
            _ => Err(format!("Setting {} not supported for XRSettings", path)),
        }
    }
}
