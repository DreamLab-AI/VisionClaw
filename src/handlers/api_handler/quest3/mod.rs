

use actix_web::{web, HttpResponse, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use crate::ok_json;

use crate::actors::messages::{GetSettings, UpdateSettings};
use crate:::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Quest3Settings {
    pub xr: Quest3XRSettings,
    pub visualisation: Quest3VisualizationSettings,
    pub performance: Quest3PerformanceSettings,
    pub interaction: Quest3InteractionSettings,
    pub gpu: Quest3GpuSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Quest3XRSettings {
    pub enabled: bool,
    pub display_mode: String, 
    pub space_type: String,   
    pub enable_hand_tracking: bool,
    pub enable_passthrough_portal: bool,
    pub passthrough_opacity: f32,
    pub passthrough_brightness: f32,
    pub passthrough_contrast: f32,
    pub enable_plane_detection: bool,
    pub enable_scene_understanding: bool,
    pub locomotion_method: String, 
    pub movement_speed: f32,
    pub interaction_distance: f32,
    pub quality: String,   
    pub refresh_rate: u32, 
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Quest3VisualizationSettings {
    pub rendering_context: String, 
    pub enable_antialiasing: bool,
    pub enable_shadows: bool,
    pub shadow_quality: String,   
    pub background_color: String, 
    pub bounds_size: f32,
    pub max_velocity: f32,
    pub physics_enabled: bool,
    pub particle_count: u32,
    pub lod_enabled: bool,
    pub culling_distance: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Quest3PerformanceSettings {
    pub target_framerate: u32,
    pub adaptive_quality: bool,
    pub thermal_throttling: bool,
    pub battery_optimization: bool,
    pub gpu_priority: String, 
    pub cpu_affinity: String, 
    pub memory_pressure_handling: String, 
    pub network_optimization: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Quest3InteractionSettings {
    pub hand_tracking_confidence: f32,
    pub gesture_recognition: bool,
    pub eye_tracking: bool,
    pub voice_commands: bool,
    pub haptic_feedback: bool,
    pub comfort_settings: ComfortSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Quest3GpuSettings {
    pub kernel_mode: String, 
    pub force_params: GpuForceParams,
    pub constraints: Vec<String>,
    pub isolation_layers: Vec<String>,
    pub trajectories_enabled: bool,
    pub clustering_enabled: bool,
    pub anomaly_detection: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GpuForceParams {
    pub repulsion: f32,
    pub attraction: f32,
    pub damping: f32,
    pub temperature: f32,
    pub max_velocity: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ComfortSettings {
    pub motion_sickness_reduction: bool,
    pub vignetting: bool,
    pub teleport_fade: bool,
    pub snap_turning: bool,
    pub comfort_mode_intensity: f32, 
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quest3DefaultsResponse {
    pub success: bool,
    pub settings: Option<Quest3Settings>,
    pub profile_name: String,
    pub optimizations_applied: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quest3CalibrationRequest {
    pub user_profile: Option<UserProfile>,
    pub environment: Option<EnvironmentProfile>,
    pub performance_target: Option<PerformanceTarget>,
    pub preferences: Option<UserPreferences>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub ipd: Option<f32>,                 
    pub height: Option<f32>,              
    pub dominant_hand: Option<String>,    
    pub experience_level: Option<String>, 
    pub motion_sensitivity: Option<f32>,  
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentProfile {
    pub lighting_conditions: Option<String>, 
    pub space_size: Option<String>,          
    pub tracking_quality: Option<String>,    
    pub wifi_quality: Option<String>,        
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceTarget {
    pub target_framerate: Option<u32>,      
    pub quality_preference: Option<String>, 
    pub battery_priority: Option<bool>,
    pub thermal_priority: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferences {
    pub comfort_level: Option<f32>,        
    pub interaction_style: Option<String>, 
    pub ar_preference: Option<bool>,       
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quest3CalibrationResponse {
    pub success: bool,
    pub calibrated_settings: Option<Quest3Settings>,
    pub calibration_id: String,
    pub applied_optimizations: Vec<OptimizationApplied>,
    pub recommendations: Vec<String>,
    pub estimated_performance: PerformanceEstimate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizationApplied {
    pub category: String,
    pub optimization: String,
    pub reason: String,
    pub impact: String, 
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceEstimate {
    pub expected_framerate: u32,
    pub battery_life_hours: f32,
    pub thermal_rating: String, 
    pub comfort_score: f32,     
    pub quality_score: f32,     
}

pub async fn get_quest3_defaults(_app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Getting Quest 3 optimized default settings");

    let default_settings = create_quest3_defaults();
    let optimizations = vec![
        "Hand tracking enabled for natural interaction".to_string(),
        "Passthrough optimized for AR mode".to_string(),
        "Performance settings tuned for 90Hz".to_string(),
        "Comfort settings configured for general users".to_string(),
        "LOD and culling optimized for Quest 3 hardware".to_string(),
    ];

    ok_json!(Quest3DefaultsResponse {
        success: true,
        settings: Some(default_settings),
        profile_name: "quest3_optimized_default".to_string(),
        optimizations_applied: optimizations,
        error: None,
    })
}

pub async fn calibrate_quest3(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<Quest3CalibrationRequest>,
) -> Result<HttpResponse> {
    info!("Calibrating Quest 3 settings for user profile");

    
    let mut calibrated_settings = create_quest3_defaults();
    let mut optimizations = Vec::new();
    let mut recommendations = Vec::new();

    
    if let Some(user_profile) = &request.user_profile {
        apply_user_profile_optimizations(
            &mut calibrated_settings,
            &mut optimizations,
            user_profile,
        );
    }

    
    if let Some(environment) = &request.environment {
        apply_environment_optimizations(&mut calibrated_settings, &mut optimizations, environment);
    }

    
    if let Some(performance_target) = &request.performance_target {
        apply_performance_optimizations(
            &mut calibrated_settings,
            &mut optimizations,
            performance_target,
        );
    }

    
    if let Some(preferences) = &request.preferences {
        apply_user_preferences(
            &mut calibrated_settings,
            &mut optimizations,
            &mut recommendations,
            preferences,
        );
    }

    
    let calibration_id = format!("quest3_cal_{}", chrono::Utc::now().timestamp());

    
    let performance_estimate = estimate_performance(&calibrated_settings);

    
    let settings_applied =
        match apply_quest3_settings_to_system(app_state, &calibrated_settings).await {
            Ok(()) => {
                info!("Quest 3 calibrated settings applied successfully");
                true
            }
            Err(e) => {
                warn!("Failed to apply Quest 3 settings to system: {}", e);
                recommendations.push(
                    "Settings generated but not applied - manual configuration may be required"
                        .to_string(),
                );
                false
            }
        };

    if !settings_applied {
        recommendations
            .push("Restart the application to ensure all settings take effect".to_string());
    }

    ok_json!(Quest3CalibrationResponse {
        success: true,
        calibrated_settings: Some(calibrated_settings),
        calibration_id,
        applied_optimizations: optimizations,
        recommendations,
        estimated_performance: performance_estimate,
        error: None,
    })
}

fn create_quest3_defaults() -> Quest3Settings {
    Quest3Settings {
        xr: Quest3XRSettings {
            enabled: true,
            display_mode: "immersive-ar".to_string(),
            space_type: "local-floor".to_string(),
            enable_hand_tracking: true,
            enable_passthrough_portal: true,
            passthrough_opacity: 1.0,
            passthrough_brightness: 1.0,
            passthrough_contrast: 1.0,
            enable_plane_detection: true,
            enable_scene_understanding: true,
            locomotion_method: "teleport".to_string(),
            movement_speed: 1.0,
            interaction_distance: 1.5,
            quality: "high".to_string(),
            refresh_rate: 90,
        },
        visualisation: Quest3VisualizationSettings {
            rendering_context: "quest3-ar".to_string(),
            enable_antialiasing: true,
            enable_shadows: true,
            shadow_quality: "medium".to_string(),
            background_color: "transparent".to_string(),
            bounds_size: 5.0,
            max_velocity: 0.01,
            physics_enabled: true,
            particle_count: 1000,
            lod_enabled: true,
            culling_distance: 50.0,
        },
        performance: Quest3PerformanceSettings {
            target_framerate: 90,
            adaptive_quality: true,
            thermal_throttling: true,
            battery_optimization: false,
            gpu_priority: "balanced".to_string(),
            cpu_affinity: "performance_cores".to_string(),
            memory_pressure_handling: "moderate".to_string(),
            network_optimization: true,
        },
        interaction: Quest3InteractionSettings {
            hand_tracking_confidence: 0.7,
            gesture_recognition: true,
            eye_tracking: false, 
            voice_commands: true,
            haptic_feedback: true,
            comfort_settings: ComfortSettings {
                motion_sickness_reduction: true,
                vignetting: true,
                teleport_fade: true,
                snap_turning: false,
                comfort_mode_intensity: 0.3,
            },
        },
        gpu: Quest3GpuSettings {
            kernel_mode: "visual_analytics".to_string(),
            force_params: GpuForceParams {
                repulsion: 150.0,
                attraction: 0.008,
                damping: 0.96,
                temperature: 0.8,
                max_velocity: 15.0,
            },
            constraints: vec!["boundary".to_string(), "collision".to_string()],
            isolation_layers: vec!["focus".to_string()],
            trajectories_enabled: false, 
            clustering_enabled: true,
            anomaly_detection: false, 
        },
    }
}

fn apply_user_profile_optimizations(
    settings: &mut Quest3Settings,
    optimizations: &mut Vec<OptimizationApplied>,
    profile: &UserProfile,
) {
    if let Some(experience) = &profile.experience_level {
        match experience.as_str() {
            "beginner" => {
                settings.interaction.comfort_settings.comfort_mode_intensity = 0.8;
                settings.interaction.comfort_settings.snap_turning = true;
                settings.xr.locomotion_method = "teleport".to_string();
                optimizations.push(OptimizationApplied {
                    category: "Comfort".to_string(),
                    optimization: "Enhanced comfort settings for beginners".to_string(),
                    reason: "Reduce motion sickness and improve accessibility".to_string(),
                    impact: "high".to_string(),
                });
            }
            "expert" => {
                settings.interaction.comfort_settings.comfort_mode_intensity = 0.1;
                settings.interaction.comfort_settings.snap_turning = false;
                settings.xr.locomotion_method = "smooth".to_string();
                settings.performance.target_framerate = 120;
                optimizations.push(OptimizationApplied {
                    category: "Performance".to_string(),
                    optimization: "High-performance settings for expert users".to_string(),
                    reason: "Maximize fidelity and responsiveness".to_string(),
                    impact: "high".to_string(),
                });
            }
            _ => {} 
        }
    }

    if let Some(motion_sensitivity) = profile.motion_sensitivity {
        if motion_sensitivity > 0.7 {
            settings
                .interaction
                .comfort_settings
                .motion_sickness_reduction = true;
            settings.interaction.comfort_settings.vignetting = true;
            settings.interaction.comfort_settings.comfort_mode_intensity = motion_sensitivity;
            optimizations.push(OptimizationApplied {
                category: "Comfort".to_string(),
                optimization: "Motion sensitivity accommodations".to_string(),
                reason: "User reported high motion sensitivity".to_string(),
                impact: "medium".to_string(),
            });
        }
    }
}

fn apply_environment_optimizations(
    settings: &mut Quest3Settings,
    optimizations: &mut Vec<OptimizationApplied>,
    environment: &EnvironmentProfile,
) {
    if let Some(lighting) = &environment.lighting_conditions {
        match lighting.as_str() {
            "dim" => {
                settings.xr.passthrough_brightness = 1.3;
                settings.xr.passthrough_contrast = 1.2;
                optimizations.push(OptimizationApplied {
                    category: "Display".to_string(),
                    optimization: "Enhanced passthrough for dim lighting".to_string(),
                    reason: "Improve visibility in low-light conditions".to_string(),
                    impact: "medium".to_string(),
                });
            }
            "bright" => {
                settings.xr.passthrough_brightness = 0.8;
                settings.xr.passthrough_contrast = 0.9;
                optimizations.push(OptimizationApplied {
                    category: "Display".to_string(),
                    optimization: "Reduced passthrough brightness for bright conditions"
                        .to_string(),
                    reason: "Prevent washout in bright lighting".to_string(),
                    impact: "medium".to_string(),
                });
            }
            _ => {} 
        }
    }

    if let Some(space_size) = &environment.space_size {
        match space_size.as_str() {
            "small" => {
                settings.visualisation.bounds_size = 2.0;
                settings.visualisation.culling_distance = 20.0;
                settings.xr.space_type = "local-floor".to_string();
            }
            "large" => {
                settings.visualisation.bounds_size = 10.0;
                settings.visualisation.culling_distance = 100.0;
                settings.xr.space_type = "bounded-floor".to_string();
            }
            _ => {} 
        }
    }

    if let Some(tracking_quality) = &environment.tracking_quality {
        if tracking_quality == "poor" {
            settings.xr.enable_hand_tracking = false;
            settings.interaction.hand_tracking_confidence = 0.9;
            optimizations.push(OptimizationApplied {
                category: "Tracking".to_string(),
                optimization: "Compensated for poor tracking conditions".to_string(),
                reason: "Improve stability with reduced tracking quality".to_string(),
                impact: "high".to_string(),
            });
        }
    }
}

fn apply_performance_optimizations(
    settings: &mut Quest3Settings,
    optimizations: &mut Vec<OptimizationApplied>,
    target: &PerformanceTarget,
) {
    if let Some(framerate) = target.target_framerate {
        settings.performance.target_framerate = framerate;
        settings.xr.refresh_rate = framerate;

        match framerate {
            120 => {
                settings.performance.gpu_priority = "performance".to_string();
                settings.visualisation.shadow_quality = "low".to_string();
                settings.visualisation.particle_count = 500;
            }
            72 => {
                settings.performance.gpu_priority = "power_save".to_string();
                settings.visualisation.shadow_quality = "high".to_string();
                settings.visualisation.particle_count = 2000;
                settings.performance.battery_optimization = true;
            }
            _ => {} 
        }
    }

    if let Some(quality_pref) = &target.quality_preference {
        match quality_pref.as_str() {
            "performance" => {
                settings.visualisation.enable_shadows = false;
                settings.visualisation.enable_antialiasing = false;
                settings.visualisation.lod_enabled = true;
                settings.performance.adaptive_quality = true;
                optimizations.push(OptimizationApplied {
                    category: "Performance".to_string(),
                    optimization: "Maximized performance settings".to_string(),
                    reason: "User prioritized performance over quality".to_string(),
                    impact: "high".to_string(),
                });
            }
            "quality" => {
                settings.visualisation.enable_shadows = true;
                settings.visualisation.shadow_quality = "high".to_string();
                settings.visualisation.enable_antialiasing = true;
                settings.visualisation.particle_count = 3000;
                settings.performance.adaptive_quality = false;
                optimizations.push(OptimizationApplied {
                    category: "Quality".to_string(),
                    optimization: "Maximized visual quality settings".to_string(),
                    reason: "User prioritized quality over performance".to_string(),
                    impact: "high".to_string(),
                });
            }
            _ => {} 
        }
    }

    if target.battery_priority.unwrap_or(false) {
        settings.performance.battery_optimization = true;
        settings.performance.thermal_throttling = true;
        settings.performance.gpu_priority = "power_save".to_string();
        optimizations.push(OptimizationApplied {
            category: "Power".to_string(),
            optimization: "Battery life optimization".to_string(),
            reason: "User prioritized battery life".to_string(),
            impact: "medium".to_string(),
        });
    }
}

fn apply_user_preferences(
    settings: &mut Quest3Settings,
    optimizations: &mut Vec<OptimizationApplied>,
    recommendations: &mut Vec<String>,
    preferences: &UserPreferences,
) {
    if let Some(comfort_level) = preferences.comfort_level {
        settings.interaction.comfort_settings.comfort_mode_intensity = comfort_level;
        if comfort_level > 0.7 {
            settings
                .interaction
                .comfort_settings
                .motion_sickness_reduction = true;
            settings.interaction.comfort_settings.vignetting = true;
            settings.interaction.comfort_settings.teleport_fade = true;
        }
    }

    if let Some(interaction_style) = &preferences.interaction_style {
        match interaction_style.as_str() {
            "hands" => {
                settings.xr.enable_hand_tracking = true;
                settings.interaction.hand_tracking_confidence = 0.6;
                settings.interaction.gesture_recognition = true;
                recommendations
                    .push("Consider enabling voice commands for backup interaction".to_string());
            }
            "controllers" => {
                settings.xr.enable_hand_tracking = false;
                settings.interaction.haptic_feedback = true;
                recommendations.push("Ensure controllers are charged and paired".to_string());
            }
            _ => {} 
        }
    }

    if preferences.ar_preference.unwrap_or(false) {
        settings.xr.display_mode = "immersive-ar".to_string();
        settings.xr.enable_passthrough_portal = true;
        settings.visualisation.background_color = "transparent".to_string();
        settings.visualisation.rendering_context = "quest3-ar".to_string();
        optimizations.push(OptimizationApplied {
            category: "Display".to_string(),
            optimization: "AR mode prioritized".to_string(),
            reason: "User prefers AR over VR experiences".to_string(),
            impact: "medium".to_string(),
        });
    }
}

fn estimate_performance(settings: &Quest3Settings) -> PerformanceEstimate {
    let mut performance_score = 1.0f32;
    let mut battery_multiplier = 1.0f32;

    
    performance_score *= match settings.performance.target_framerate {
        120 => 0.7, 
        90 => 1.0,  
        72 => 1.3,  
        _ => 1.0,
    };

    
    if settings.visualisation.enable_shadows {
        performance_score *= 0.9;
        battery_multiplier *= 1.1;
    }
    if settings.visualisation.enable_antialiasing {
        performance_score *= 0.95;
        battery_multiplier *= 1.05;
    }

    
    if settings.xr.enable_hand_tracking {
        performance_score *= 0.98;
        battery_multiplier *= 1.02;
    }

    
    if settings.performance.battery_optimization {
        battery_multiplier *= 0.8;
    }

    let expected_framerate =
        (settings.performance.target_framerate as f32 * performance_score) as u32;
    let battery_life = 2.5 * battery_multiplier; 

    let thermal_rating = if performance_score < 0.8 && settings.performance.target_framerate >= 90 {
        "hot"
    } else if performance_score < 0.9 {
        "warm"
    } else {
        "cool"
    };

    let comfort_score = settings.interaction.comfort_settings.comfort_mode_intensity;
    let quality_score = if settings.visualisation.enable_shadows
        && settings.visualisation.enable_antialiasing
    {
        0.9
    } else if settings.visualisation.enable_shadows || settings.visualisation.enable_antialiasing {
        0.7
    } else {
        0.5
    };

    PerformanceEstimate {
        expected_framerate,
        battery_life_hours: battery_life,
        thermal_rating: thermal_rating.to_string(),
        comfort_score,
        quality_score,
    }
}

async fn apply_quest3_settings_to_system(
    app_state: web::Data<AppState>,
    quest3_settings: &Quest3Settings,
) -> Result<(), String> {
    
    let mut settings = app_state
        .settings_addr
        .send(GetSettings)
        .await
        .map_err(|e| format!("Failed to get settings: {}", e))?
        .map_err(|e| format!("Settings error: {}", e))?;

    
    settings.xr.enabled = Some(quest3_settings.xr.enabled);

    
    settings.xr.mode = Some(quest3_settings.xr.display_mode.clone());
    settings.xr.space_type = quest3_settings.xr.space_type.clone();

    settings.xr.enable_hand_tracking = quest3_settings.xr.enable_hand_tracking;
    settings.xr.enable_passthrough_portal = quest3_settings.xr.enable_passthrough_portal;
    settings.xr.passthrough_opacity = quest3_settings.xr.passthrough_opacity;
    settings.xr.passthrough_brightness = quest3_settings.xr.passthrough_brightness;
    settings.xr.passthrough_contrast = quest3_settings.xr.passthrough_contrast;
    settings.xr.movement_speed = quest3_settings.xr.movement_speed;
    settings.xr.locomotion_method = quest3_settings.xr.locomotion_method.clone();
    settings.xr.interaction_radius = quest3_settings.xr.interaction_distance;

    
    settings.visualisation.graphs.logseq.physics.bounds_size =
        quest3_settings.visualisation.bounds_size;
    settings.visualisation.graphs.logseq.physics.max_velocity =
        quest3_settings.visualisation.max_velocity;
    settings.visualisation.graphs.logseq.physics.enabled =
        quest3_settings.visualisation.physics_enabled;

    settings.visualisation.graphs.visionflow.physics.bounds_size =
        quest3_settings.visualisation.bounds_size;
    settings
        .visualisation
        .graphs
        .visionflow
        .physics
        .max_velocity = quest3_settings.visualisation.max_velocity;
    settings.visualisation.graphs.visionflow.physics.enabled =
        quest3_settings.visualisation.physics_enabled;

    
    app_state
        .settings_addr
        .send(UpdateSettings { settings })
        .await
        .map_err(|e| format!("Failed to update settings: {}", e))?
        .map_err(|e| format!("Settings update error: {}", e))?;

    Ok(())
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/quest3")
            .route("/defaults", web::get().to(get_quest3_defaults))
            .route("/calibrate", web::post().to(calibrate_quest3)),
    );
}

