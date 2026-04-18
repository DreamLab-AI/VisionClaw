// Conversion functions between AppFullSettings config types and DTOs

use crate::config::AppFullSettings;
use super::types::*;

// Conversion functions between AppFullSettings and DTOs
impl From<&AppFullSettings> for SettingsResponseDTO {
    fn from(settings: &AppFullSettings) -> Self {
        Self {
            visualisation: (&settings.visualisation).into(),
            system: (&settings.system).into(),
            xr: (&settings.xr).into(),
            auth: (&settings.auth).into(),
            ragflow: settings.ragflow.as_ref().map(|r| r.into()),
            perplexity: settings.perplexity.as_ref().map(|p| p.into()),
            openai: settings.openai.as_ref().map(|o| o.into()),
            kokoro: settings.kokoro.as_ref().map(|k| k.into()),
            whisper: settings.whisper.as_ref().map(|w| w.into()),
        }
    }
}

// Implement all the necessary From conversions for nested structures
impl From<&crate::config::VisualisationSettings> for VisualisationSettingsDTO {
    fn from(settings: &crate::config::VisualisationSettings) -> Self {
        Self {
            rendering: (&settings.rendering).into(),
            animations: (&settings.animations).into(),
            glow: (&settings.glow).into(),
            hologram: (&settings.hologram).into(),
            graphs: (&settings.graphs).into(),
            camera: settings.camera.as_ref().map(|c| c.into()),
            space_pilot: settings.space_pilot.as_ref().map(|sp| sp.into()),
        }
    }
}

impl From<&crate::config::RenderingSettings> for RenderingSettingsDTO {
    fn from(settings: &crate::config::RenderingSettings) -> Self {

        let dev_config = crate::config::dev_config::rendering();
        let agent_colors = Some(AgentColorsDTO {
            coordinator: dev_config.agent_colors.coordinator.clone(),
            coder: dev_config.agent_colors.coder.clone(),
            architect: dev_config.agent_colors.architect.clone(),
            analyst: dev_config.agent_colors.analyst.clone(),
            tester: dev_config.agent_colors.tester.clone(),
            researcher: dev_config.agent_colors.researcher.clone(),
            reviewer: dev_config.agent_colors.reviewer.clone(),
            optimizer: dev_config.agent_colors.optimizer.clone(),
            documenter: dev_config.agent_colors.documenter.clone(),
            queen: "#FFD700".to_string(),
            default: dev_config.agent_colors.default.clone(),
        });

        Self {
            ambient_light_intensity: settings.ambient_light_intensity,
            background_color: settings.background_color.clone(),
            directional_light_intensity: settings.directional_light_intensity,
            enable_ambient_occlusion: settings.enable_ambient_occlusion,
            enable_antialiasing: settings.enable_antialiasing,
            enable_shadows: settings.enable_shadows,
            environment_intensity: settings.environment_intensity,
            shadow_map_size: settings.shadow_map_size.clone(),
            shadow_bias: settings.shadow_bias,
            context: settings.context.clone(),
            agent_colors,
        }
    }
}

impl From<&crate::config::AnimationSettings> for AnimationSettingsDTO {
    fn from(settings: &crate::config::AnimationSettings) -> Self {
        Self {
            enable_motion_blur: settings.enable_motion_blur,
            enable_node_animations: settings.enable_node_animations,
            motion_blur_strength: settings.motion_blur_strength,
            selection_wave_enabled: settings.selection_wave_enabled,
            pulse_enabled: settings.pulse_enabled,
            pulse_speed: settings.pulse_speed,
            pulse_strength: settings.pulse_strength,
            wave_speed: settings.wave_speed,
        }
    }
}

impl From<&crate::config::GlowSettings> for GlowSettingsDTO {
    fn from(settings: &crate::config::GlowSettings) -> Self {
        Self {
            enabled: settings.enabled,
            intensity: settings.intensity,
            radius: settings.radius,
            threshold: settings.threshold,
            diffuse_strength: settings.diffuse_strength,
            atmospheric_density: settings.atmospheric_density,
            volumetric_intensity: settings.volumetric_intensity,
            base_color: settings.base_color.clone(),
            emission_color: settings.emission_color.clone(),
            opacity: settings.opacity,
            pulse_speed: settings.pulse_speed,
            flow_speed: settings.flow_speed,
            node_glow_strength: settings.node_glow_strength,
            edge_glow_strength: settings.edge_glow_strength,
            environment_glow_strength: settings.environment_glow_strength,
        }
    }
}

impl From<&crate::config::HologramSettings> for HologramSettingsDTO {
    fn from(settings: &crate::config::HologramSettings) -> Self {
        Self {
            ring_count: settings.ring_count,
            ring_color: settings.ring_color.clone(),
            ring_opacity: settings.ring_opacity,
            sphere_sizes: settings.sphere_sizes.clone(),
            ring_rotation_speed: settings.ring_rotation_speed,
            enable_buckminster: settings.enable_buckminster,
            buckminster_size: settings.buckminster_size,
            buckminster_opacity: settings.buckminster_opacity,
            enable_geodesic: settings.enable_geodesic,
            geodesic_size: settings.geodesic_size,
            geodesic_opacity: settings.geodesic_opacity,
            enable_triangle_sphere: settings.enable_triangle_sphere,
            triangle_sphere_size: settings.triangle_sphere_size,
            triangle_sphere_opacity: settings.triangle_sphere_opacity,
            global_rotation_speed: settings.global_rotation_speed,
        }
    }
}

impl From<&crate::config::GraphsSettings> for GraphsSettingsDTO {
    fn from(settings: &crate::config::GraphsSettings) -> Self {
        Self {
            logseq: (&settings.logseq).into(),
            visionflow: (&settings.visionflow).into(),
        }
    }
}

impl From<&crate::config::GraphSettings> for GraphSettingsDTO {
    fn from(settings: &crate::config::GraphSettings) -> Self {
        Self {
            nodes: (&settings.nodes).into(),
            edges: (&settings.edges).into(),
            labels: (&settings.labels).into(),
            physics: (&settings.physics).into(),
        }
    }
}

impl From<&crate::config::NodeSettings> for NodeSettingsDTO {
    fn from(settings: &crate::config::NodeSettings) -> Self {
        Self {
            base_color: settings.base_color.clone(),
            metalness: settings.metalness,
            opacity: settings.opacity,
            roughness: settings.roughness,
            node_size: settings.node_size,
            quality: settings.quality.clone(),
            enable_instancing: settings.enable_instancing,
            enable_hologram: settings.enable_hologram,
            enable_metadata_shape: settings.enable_metadata_shape,
            enable_metadata_visualisation: settings.enable_metadata_visualisation,
        }
    }
}

impl From<&crate::config::EdgeSettings> for EdgeSettingsDTO {
    fn from(settings: &crate::config::EdgeSettings) -> Self {
        Self {
            arrow_size: settings.arrow_size,
            base_width: settings.base_width,
            color: settings.color.clone(),
            enable_arrows: settings.enable_arrows,
            opacity: settings.opacity,
            width_range: settings.width_range.clone(),
            quality: settings.quality.clone(),
        }
    }
}

impl From<&crate::config::LabelSettings> for LabelSettingsDTO {
    fn from(settings: &crate::config::LabelSettings) -> Self {
        Self {
            desktop_font_size: settings.desktop_font_size,
            enable_labels: settings.enable_labels,
            text_color: settings.text_color.clone(),
            text_outline_color: settings.text_outline_color.clone(),
            text_outline_width: settings.text_outline_width,
            text_resolution: settings.text_resolution,
            text_padding: settings.text_padding,
            billboard_mode: settings.billboard_mode.clone(),
            show_metadata: settings.show_metadata,
            max_label_width: settings.max_label_width,
        }
    }
}

impl From<&crate::config::PhysicsSettings> for PhysicsSettingsDTO {
    fn from(settings: &crate::config::PhysicsSettings) -> Self {
        Self {
            auto_balance: settings.auto_balance,
            auto_balance_interval_ms: settings.auto_balance_interval_ms,
            auto_balance_config: (&settings.auto_balance_config).into(),
            spring_k: settings.spring_k,
            bounds_size: settings.bounds_size,
            separation_radius: settings.separation_radius,
            damping: settings.damping,
            enable_bounds: settings.enable_bounds,
            enabled: settings.enabled,
            iterations: settings.iterations,
            max_velocity: settings.max_velocity,
            max_force: settings.max_force,
            repel_k: settings.repel_k,
            boundary_damping: settings.boundary_damping,
            update_threshold: settings.update_threshold,
            dt: settings.dt,
            temperature: settings.temperature,
            gravity: settings.gravity,
            alignment_strength: settings.alignment_strength,
            cluster_strength: settings.cluster_strength,
            compute_mode: settings.compute_mode,
            rest_length: settings.rest_length,
            repulsion_cutoff: settings.repulsion_cutoff,
            repulsion_softening_epsilon: settings.repulsion_softening_epsilon,
            center_gravity_k: settings.center_gravity_k,
            grid_cell_size: settings.grid_cell_size,
            warmup_iterations: settings.warmup_iterations,
            cooling_rate: settings.cooling_rate,
            boundary_extreme_multiplier: settings.boundary_extreme_multiplier,
            boundary_extreme_force_multiplier: settings.boundary_extreme_force_multiplier,
            boundary_velocity_damping: settings.boundary_velocity_damping,
            min_distance: settings.min_distance,
            max_repulsion_dist: settings.max_repulsion_dist,
            boundary_margin: settings.boundary_margin,
            boundary_force_strength: settings.boundary_force_strength,
            clustering_algorithm: settings.clustering_algorithm.clone(),
            cluster_count: settings.cluster_count,
            clustering_resolution: settings.clustering_resolution,
            clustering_iterations: settings.clustering_iterations,
            graph_separation_x: settings.graph_separation_x,
            physicality_strength: settings.physicality_strength,
            role_strength: settings.role_strength,
            maturity_strength: settings.maturity_strength,
        }
    }
}

impl From<&crate::config::AutoBalanceConfig> for AutoBalanceConfigDTO {
    fn from(settings: &crate::config::AutoBalanceConfig) -> Self {
        Self {
            stability_variance_threshold: settings.stability_variance_threshold,
            stability_frame_count: settings.stability_frame_count,
            clustering_distance_threshold: settings.clustering_distance_threshold,
            bouncing_node_percentage: settings.bouncing_node_percentage,
            boundary_min_distance: settings.boundary_min_distance,
            boundary_max_distance: settings.boundary_max_distance,
            extreme_distance_threshold: settings.extreme_distance_threshold,
            explosion_distance_threshold: settings.explosion_distance_threshold,
            spreading_distance_threshold: settings.spreading_distance_threshold,
            oscillation_detection_frames: settings.oscillation_detection_frames,
            oscillation_change_threshold: settings.oscillation_change_threshold,
            min_oscillation_changes: settings.min_oscillation_changes,
            grid_cell_size_min: settings.grid_cell_size_min,
            grid_cell_size_max: settings.grid_cell_size_max,
            repulsion_cutoff_min: settings.repulsion_cutoff_min,
            repulsion_cutoff_max: settings.repulsion_cutoff_max,
            repulsion_softening_min: settings.repulsion_softening_min,
            repulsion_softening_max: settings.repulsion_softening_max,
            center_gravity_min: settings.center_gravity_min,
            center_gravity_max: settings.center_gravity_max,
            spatial_hash_efficiency_threshold: settings.spatial_hash_efficiency_threshold,
            cluster_density_threshold: settings.cluster_density_threshold,
            numerical_instability_threshold: settings.numerical_instability_threshold,
        }
    }
}

impl From<&crate::config::CameraSettings> for CameraSettingsDTO {
    fn from(settings: &crate::config::CameraSettings) -> Self {
        Self {
            fov: settings.fov,
            near: settings.near,
            far: settings.far,
            position: (&settings.position).into(),
            look_at: (&settings.look_at).into(),
        }
    }
}

impl From<&crate::config::Position> for PositionDTO {
    fn from(pos: &crate::config::Position) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }
}

impl From<&crate::config::SpacePilotSettings> for SpacePilotSettingsDTO {
    fn from(settings: &crate::config::SpacePilotSettings) -> Self {
        Self {
            enabled: settings.enabled,
            mode: settings.mode.clone(),
            sensitivity: (&settings.sensitivity).into(),
            smoothing: settings.smoothing,
            deadzone: settings.deadzone,
            button_functions: settings.button_functions.clone(),
        }
    }
}

impl From<&crate::config::Sensitivity> for SensitivityDTO {
    fn from(sens: &crate::config::Sensitivity) -> Self {
        Self {
            translation: sens.translation,
            rotation: sens.rotation,
        }
    }
}

impl From<&crate::config::SystemSettings> for SystemSettingsDTO {
    fn from(settings: &crate::config::SystemSettings) -> Self {
        Self {
            network: (&settings.network).into(),
            websocket: (&settings.websocket).into(),
            security: (&settings.security).into(),
            debug: (&settings.debug).into(),
            persist_settings: settings.persist_settings,
            custom_backend_url: settings.custom_backend_url.clone(),
        }
    }
}

impl From<&crate::config::NetworkSettings> for NetworkSettingsDTO {
    fn from(settings: &crate::config::NetworkSettings) -> Self {
        Self {
            bind_address: settings.bind_address.clone(),
            domain: settings.domain.clone(),
            enable_http2: settings.enable_http2,
            enable_rate_limiting: settings.enable_rate_limiting,
            enable_tls: settings.enable_tls,
            max_request_size: settings.max_request_size,
            min_tls_version: settings.min_tls_version.clone(),
            port: settings.port,
            rate_limit_requests: settings.rate_limit_requests,
            rate_limit_window: settings.rate_limit_window,
            tunnel_id: settings.tunnel_id.clone(),
            api_client_timeout: settings.api_client_timeout,
            enable_metrics: settings.enable_metrics,
            max_concurrent_requests: settings.max_concurrent_requests,
            max_retries: settings.max_retries,
            metrics_port: settings.metrics_port,
            retry_delay: settings.retry_delay,
        }
    }
}

impl From<&crate::config::WebSocketSettings> for WebSocketSettingsDTO {
    fn from(settings: &crate::config::WebSocketSettings) -> Self {
        Self {
            binary_chunk_size: settings.binary_chunk_size,
            binary_update_rate: settings.binary_update_rate,
            min_update_rate: settings.min_update_rate,
            max_update_rate: settings.max_update_rate,
            motion_threshold: settings.motion_threshold,
            motion_damping: settings.motion_damping,
            binary_message_version: settings.binary_message_version,
            compression_enabled: settings.compression_enabled,
            compression_threshold: settings.compression_threshold,
            heartbeat_interval: settings.heartbeat_interval,
            heartbeat_timeout: settings.heartbeat_timeout,
            max_connections: settings.max_connections,
            max_message_size: settings.max_message_size,
            reconnect_attempts: settings.reconnect_attempts,
            reconnect_delay: settings.reconnect_delay,
            update_rate: settings.update_rate,
        }
    }
}

impl From<&crate::config::SecuritySettings> for SecuritySettingsDTO {
    fn from(settings: &crate::config::SecuritySettings) -> Self {
        Self {
            allowed_origins: settings.allowed_origins.clone(),
            audit_log_path: settings.audit_log_path.clone(),
            cookie_httponly: settings.cookie_httponly,
            cookie_samesite: settings.cookie_samesite.clone(),
            cookie_secure: settings.cookie_secure,
            csrf_token_timeout: settings.csrf_token_timeout,
            enable_audit_logging: settings.enable_audit_logging,
            enable_request_validation: settings.enable_request_validation,
            session_timeout: settings.session_timeout,
        }
    }
}

impl From<&crate::config::DebugSettings> for DebugSettingsDTO {
    fn from(settings: &crate::config::DebugSettings) -> Self {
        Self {
            enabled: settings.enabled,
        }
    }
}

impl From<&crate::config::XRSettings> for XRSettingsDTO {
    fn from(settings: &crate::config::XRSettings) -> Self {
        Self {
            enabled: settings.enabled,
            client_side_enable_xr: settings.client_side_enable_xr,
            mode: settings.mode.clone(),
            room_scale: settings.room_scale,
            space_type: settings.space_type.clone(),
            quality: settings.quality.clone(),
            render_scale: settings.render_scale,
            interaction_distance: settings.interaction_distance,
            locomotion_method: settings.locomotion_method.clone(),
            teleport_ray_color: settings.teleport_ray_color.clone(),
            controller_ray_color: settings.controller_ray_color.clone(),
            controller_model: settings.controller_model.clone(),
            enable_hand_tracking: settings.enable_hand_tracking,
            hand_mesh_enabled: settings.hand_mesh_enabled,
            hand_mesh_color: settings.hand_mesh_color.clone(),
            hand_mesh_opacity: settings.hand_mesh_opacity,
            hand_point_size: settings.hand_point_size,
            hand_ray_enabled: settings.hand_ray_enabled,
            hand_ray_color: settings.hand_ray_color.clone(),
            hand_ray_width: settings.hand_ray_width,
            gesture_smoothing: settings.gesture_smoothing,
            enable_haptics: settings.enable_haptics,
            haptic_intensity: settings.haptic_intensity,
            drag_threshold: settings.drag_threshold,
            pinch_threshold: settings.pinch_threshold,
            rotation_threshold: settings.rotation_threshold,
            interaction_radius: settings.interaction_radius,
            movement_speed: settings.movement_speed,
            dead_zone: settings.dead_zone,
            movement_axes: (&settings.movement_axes).into(),
            enable_light_estimation: settings.enable_light_estimation,
            enable_plane_detection: settings.enable_plane_detection,
            enable_scene_understanding: settings.enable_scene_understanding,
            plane_color: settings.plane_color.clone(),
            plane_opacity: settings.plane_opacity,
            plane_detection_distance: settings.plane_detection_distance,
            show_plane_overlay: settings.show_plane_overlay,
            snap_to_floor: settings.snap_to_floor,
            enable_passthrough_portal: settings.enable_passthrough_portal,
            passthrough_opacity: settings.passthrough_opacity,
            passthrough_brightness: settings.passthrough_brightness,
            passthrough_contrast: settings.passthrough_contrast,
            portal_size: settings.portal_size,
            portal_edge_color: settings.portal_edge_color.clone(),
            portal_edge_width: settings.portal_edge_width,
        }
    }
}

impl From<&crate::config::MovementAxes> for MovementAxesDTO {
    fn from(axes: &crate::config::MovementAxes) -> Self {
        Self {
            horizontal: axes.horizontal,
            vertical: axes.vertical,
        }
    }
}

impl From<&crate::config::AuthSettings> for AuthSettingsDTO {
    fn from(settings: &crate::config::AuthSettings) -> Self {
        Self {
            enabled: settings.enabled,
            provider: settings.provider.clone(),
            required: settings.required,
        }
    }
}

impl From<&crate::config::RagFlowSettings> for RagFlowSettingsDTO {
    fn from(settings: &crate::config::RagFlowSettings) -> Self {
        Self {
            api_key: settings.api_key.clone(),
            agent_id: settings.agent_id.clone(),
            api_base_url: settings.api_base_url.clone(),
            timeout: settings.timeout,
            max_retries: settings.max_retries,
            chat_id: settings.chat_id.clone(),
        }
    }
}

impl From<&crate::config::PerplexitySettings> for PerplexitySettingsDTO {
    fn from(settings: &crate::config::PerplexitySettings) -> Self {
        Self {
            api_key: settings.api_key.clone(),
            model: settings.model.clone(),
            api_url: settings.api_url.clone(),
            max_tokens: settings.max_tokens,
            temperature: settings.temperature,
            top_p: settings.top_p,
            presence_penalty: settings.presence_penalty,
            frequency_penalty: settings.frequency_penalty,
            timeout: settings.timeout,
            rate_limit: settings.rate_limit,
        }
    }
}

impl From<&crate::config::OpenAISettings> for OpenAISettingsDTO {
    fn from(settings: &crate::config::OpenAISettings) -> Self {
        Self {
            api_key: settings.api_key.clone(),
            base_url: settings.base_url.clone(),
            timeout: settings.timeout,
            rate_limit: settings.rate_limit,
        }
    }
}

impl From<&crate::config::KokoroSettings> for KokoroSettingsDTO {
    fn from(settings: &crate::config::KokoroSettings) -> Self {
        Self {
            api_url: settings.api_url.clone(),
            default_voice: settings.default_voice.clone(),
            default_format: settings.default_format.clone(),
            default_speed: settings.default_speed,
            timeout: settings.timeout,
            stream: settings.stream,
            return_timestamps: settings.return_timestamps,
            sample_rate: settings.sample_rate,
        }
    }
}

impl From<&crate::config::WhisperSettings> for WhisperSettingsDTO {
    fn from(settings: &crate::config::WhisperSettings) -> Self {
        Self {
            api_url: settings.api_url.clone(),
            default_model: settings.default_model.clone(),
            default_language: settings.default_language.clone(),
            timeout: settings.timeout,
            temperature: settings.temperature,
            return_timestamps: settings.return_timestamps,
            vad_filter: settings.vad_filter,
            word_timestamps: settings.word_timestamps,
            initial_prompt: settings.initial_prompt.clone(),
        }
    }
}
