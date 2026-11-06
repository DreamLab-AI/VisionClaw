use serde_json::Value;

pub(crate) fn convert_empty_strings_to_null(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let new_map = map
                .into_iter()
                .map(|(k, v)| {
                    let new_v = match v {
                        Value::String(s) if s.is_empty() => {
                            let required_string_fields = vec![
                                "base_color",
                                "color",
                                "background_color",
                                "text_color",
                                "text_outline_color",
                                "billboard_mode",
                                "quality",
                                "mode",
                                "context",
                                "cookie_samesite",
                                "audit_log_path",
                                "bind_address",
                                "domain",
                                "min_tls_version",
                                "tunnel_id",
                                "provider",
                                "ring_color",
                                "hand_mesh_color",
                                "hand_ray_color",
                                "teleport_ray_color",
                                "controller_ray_color",
                                "plane_color",
                                "portal_edge_color",
                                "space_type",
                                "locomotion_method",
                            ];

                            if required_string_fields.contains(&k.as_str()) {
                                Value::String(s)
                            } else {
                                Value::Null
                            }
                        }
                        Value::Object(_) => convert_empty_strings_to_null(v),
                        Value::Array(_) => convert_empty_strings_to_null(v),
                        _ => v,
                    };
                    (k, new_v)
                })
                .collect();
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.into_iter().map(convert_empty_strings_to_null).collect())
        }
        _ => value,
    }
}

pub(crate) fn merge_json_values(base: Value, update: Value) -> Value {
    use serde_json::map::Entry;

    match (base, update) {
        (Value::Object(mut base_map), Value::Object(update_map)) => {
            for (key, update_value) in update_map {
                match base_map.entry(key) {
                    Entry::Occupied(mut entry) => {
                        let merged = merge_json_values(entry.get().clone(), update_value);
                        entry.insert(merged);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(update_value);
                    }
                }
            }
            Value::Object(base_map)
        }
        (_, update) => update,
    }
}

pub(crate) static FIELD_MAPPINGS: std::sync::LazyLock<
    std::collections::HashMap<&'static str, &'static str>,
> = std::sync::LazyLock::new(|| {
    let mut field_mappings = std::collections::HashMap::new();

    field_mappings.insert("base_color", "baseColor");
    field_mappings.insert("emission_color", "emissionColor");
    field_mappings.insert("node_size", "nodeSize");
    field_mappings.insert("enable_instancing", "enableInstancing");
    field_mappings.insert("enable_hologram", "enableHologram");
    field_mappings.insert(
        "enable_metadata_visualisation",
        "enableMetadataVisualisation",
    );
    field_mappings.insert("arrow_size", "arrowSize");
    field_mappings.insert("base_width", "baseWidth");
    field_mappings.insert("edge_color", "color");
    field_mappings.insert("edge_opacity", "opacity");
    field_mappings.insert("edge_width", "edgeWidth");
    field_mappings.insert("enable_arrows", "enableArrows");
    field_mappings.insert("width_range", "widthRange");
    field_mappings.insert("ambient_light_intensity", "ambientLightIntensity");
    field_mappings.insert("background_color", "backgroundColor");
    field_mappings.insert("directional_light_intensity", "directionalLightIntensity");
    field_mappings.insert("enable_ambient_occlusion", "enableAmbientOcclusion");
    field_mappings.insert("enable_antialiasing", "enableAntialiasing");
    field_mappings.insert("enable_shadows", "enableShadows");
    field_mappings.insert("environment_intensity", "environmentIntensity");
    field_mappings.insert("shadow_map_size", "shadowMapSize");
    field_mappings.insert("shadow_bias", "shadowBias");
    field_mappings.insert("enable_motion_blur", "enableMotionBlur");
    field_mappings.insert("enable_node_animations", "enableNodeAnimations");
    field_mappings.insert("motion_blur_strength", "motionBlurStrength");
    field_mappings.insert("animation_speed", "animationSpeed");
    field_mappings.insert(
        "equilibrium_velocity_threshold",
        "equilibriumVelocityThreshold",
    );
    field_mappings.insert("equilibrium_check_frames", "equilibriumCheckFrames");
    field_mappings.insert("equilibrium_energy_threshold", "equilibriumEnergyThreshold");
    field_mappings.insert("pause_on_equilibrium", "pauseOnEquilibrium");
    field_mappings.insert("resume_on_interaction", "resumeOnInteraction");
    field_mappings.insert("stability_variance_threshold", "stabilityVarianceThreshold");
    field_mappings.insert("stability_frame_count", "stabilityFrameCount");
    field_mappings.insert(
        "clustering_distance_threshold",
        "clusteringDistanceThreshold",
    );
    field_mappings.insert("clustering_hysteresis_buffer", "clusteringHysteresisBuffer");
    field_mappings.insert("bouncing_node_percentage", "bouncingNodePercentage");
    field_mappings.insert("boundary_min_distance", "boundaryMinDistance");
    field_mappings.insert("boundary_max_distance", "boundaryMaxDistance");
    field_mappings.insert("extreme_distance_threshold", "extremeDistanceThreshold");
    field_mappings.insert("explosion_distance_threshold", "explosionDistanceThreshold");
    field_mappings.insert("spreading_distance_threshold", "spreadingDistanceThreshold");
    field_mappings.insert("spreading_hysteresis_buffer", "spreadingHysteresisBuffer");
    field_mappings.insert("oscillation_detection_frames", "oscillationDetectionFrames");
    field_mappings.insert("oscillation_change_threshold", "oscillationChangeThreshold");
    field_mappings.insert("min_oscillation_changes", "minOscillationChanges");
    field_mappings.insert("parameter_adjustment_rate", "parameterAdjustmentRate");
    field_mappings.insert("max_adjustment_factor", "maxAdjustmentFactor");
    field_mappings.insert("min_adjustment_factor", "minAdjustmentFactor");
    field_mappings.insert("adjustment_cooldown_ms", "adjustmentCooldownMs");
    field_mappings.insert("state_change_cooldown_ms", "stateChangeCooldownMs");
    field_mappings.insert("parameter_dampening_factor", "parameterDampeningFactor");
    field_mappings.insert("hysteresis_delay_frames", "hysteresisDelayFrames");
    field_mappings.insert("grid_cell_size_min", "gridCellSizeMin");
    field_mappings.insert("grid_cell_size_max", "gridCellSizeMax");
    field_mappings.insert("repulsion_cutoff_min", "repulsionCutoffMin");
    field_mappings.insert("repulsion_cutoff_max", "repulsionCutoffMax");
    field_mappings.insert("repulsion_softening_min", "repulsionSofteningMin");
    field_mappings.insert("repulsion_softening_max", "repulsionSofteningMax");
    field_mappings.insert("center_gravity_min", "centerGravityMin");
    field_mappings.insert("center_gravity_max", "centerGravityMax");
    field_mappings.insert(
        "spatial_hash_efficiency_threshold",
        "spatialHashEfficiencyThreshold",
    );
    field_mappings.insert("cluster_density_threshold", "clusterDensityThreshold");
    field_mappings.insert(
        "numerical_instability_threshold",
        "numericalInstabilityThreshold",
    );
    field_mappings.insert("bounds_size", "boundsSize");
    field_mappings.insert("separation_radius", "separationRadius");
    field_mappings.insert("enable_bounds", "enableBounds");
    field_mappings.insert("max_velocity", "maxVelocity");
    field_mappings.insert("max_force", "maxForce");
    field_mappings.insert("repel_k", "repelK");
    field_mappings.insert("spring_k", "springK");
    field_mappings.insert("boundary_damping", "boundaryDamping");
    field_mappings.insert("alignment_strength", "alignmentStrength");
    field_mappings.insert("cluster_strength", "clusterStrength");
    field_mappings.insert("compute_mode", "computeMode");
    field_mappings.insert("rest_length", "restLength");
    field_mappings.insert("repulsion_cutoff", "repulsionCutoff");
    field_mappings.insert("repulsion_softening_epsilon", "repulsionSofteningEpsilon");
    field_mappings.insert("center_gravity_k", "centerGravityK");
    field_mappings.insert("grid_cell_size", "gridCellSize");
    field_mappings.insert("warmup_iterations", "warmupIterations");
    field_mappings.insert("cooling_rate", "coolingRate");
    field_mappings.insert("boundary_extreme_multiplier", "boundaryExtremeMultiplier");
    field_mappings.insert(
        "boundary_extreme_force_multiplier",
        "boundaryExtremeForceMultiplier",
    );
    field_mappings.insert("boundary_velocity_damping", "boundaryVelocityDamping");
    field_mappings.insert("min_distance", "minDistance");
    field_mappings.insert("max_repulsion_dist", "maxRepulsionDist");
    field_mappings.insert("host_port", "hostPort");
    field_mappings.insert("log_level", "logLevel");
    field_mappings.insert("persist_settings", "persistSettings");
    field_mappings.insert("gpu_memory_limit", "gpuMemoryLimit");

    field_mappings
});

pub(crate) fn normalize_field_names_to_camel_case(
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    normalize_object_fields(value, &FIELD_MAPPINGS)
}

fn normalize_object_fields(
    value: serde_json::Value,
    mappings: &std::collections::HashMap<&str, &str>,
) -> Result<serde_json::Value, String> {
    match value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();

            for (key, val) in map {
                let normalized_key = if let Some(&camel_case_key) = mappings.get(key.as_str()) {
                    camel_case_key.to_string()
                } else {
                    key
                };

                let normalized_value = normalize_object_fields(val, mappings)?;
                new_map.insert(normalized_key, normalized_value);
            }

            Ok(Value::Object(new_map))
        }
        Value::Array(arr) => {
            let normalized_array: Result<Vec<Value>, String> = arr
                .into_iter()
                .map(|item| normalize_object_fields(item, mappings))
                .collect();
            Ok(Value::Array(normalized_array?))
        }
        _ => Ok(value),
    }
}
