use actix::prelude::*;
use log::{error, info, warn};

use super::types::SocketFlowServer;
use crate::adapters::sqlite_settings_repository::CURRENT_OWNER_PUBKEY;
use crate::ports::settings_repository::{SettingValue, SettingsRepository};
use crate::settings::api::settings_routes::UserFilter;

/// Handle "authenticate" message -- NIP-98 and legacy token/pubkey paths.
pub(crate) fn handle_authenticate(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    info!("Client sent authenticate message");

    // LAN-local full dev mode (VISIONCLAW_DEV_MODE=1): accept the authenticate
    // frame as dev admin with no NIP-98/token verification — the WS parity of the
    // REST full bypass. This grants power_user the moment the client sends its
    // `authenticate` frame (the Godot XR client always does), NOT passively at
    // TCP-connect; a client that never authenticates stays anon, same as normal
    // mode. The 403-ing HUD write buttons are REST and are covered unconditionally
    // by `verify_access` regardless of this WS path — this only lets the owner's
    // private nodes render and ungates WS-side writes (filter_update). Dev builds
    // only; stripped and boot-refused in release. See `utils::auth::dev_full_bypass_active`.
    #[cfg(any(debug_assertions, feature = "dev-auth"))]
    {
        if crate::utils::auth::dev_full_bypass_active() {
            let pubkey = crate::utils::auth::DEV_MODE_PUBKEY.to_string();
            act.pubkey = Some(pubkey.clone());
            act.is_power_user = true;
            // identity changed — invalidate any in-flight (pre-auth) sync.
            act.bump_sync_generation();
            if let Some(cid) = act.client_id {
                use crate::actors::messages::AuthenticateClient;
                act.client_manager_addr.do_send(AuthenticateClient {
                    client_id: cid,
                    pubkey: pubkey.clone(),
                    is_power_user: true,
                    ephemeral: false,
                });
            }
            let response = serde_json::json!({
                "type": "authenticate_success",
                "pubkey": pubkey,
                "is_power_user": true,
                "timestamp": chrono::Utc::now().timestamp_millis()
            });
            if let Ok(msg_str) = serde_json::to_string(&response) {
                ctx.text(msg_str);
            }
            info!(
                "dev-mode: VISIONCLAW_DEV_MODE full bypass — WS authenticated as {} (power_user)",
                pubkey
            );
            // re-push a fresh filtered sync now that pubkey is set.
            act.send_full_state_sync(ctx);
            return;
        }
    }

    if let Some(event_b64) = msg.get("event").and_then(|e| e.as_str()) {
        // --- NIP-98 path: { type: "authenticate", event: "<base64>" } ---
        let nostr_service = act.app_state.nostr_service.clone();
        let client_id = act.client_id;
        let cm_addr = act.client_manager_addr.clone();
        let auth_header = format!("Nostr {}", event_b64);
        let ws_url = act.connection_url.clone();

        ctx.spawn(
            actix::fut::wrap_future::<_, SocketFlowServer>(async move {
                if let Some(ref ns) = nostr_service {
                    match ns
                        .verify_nip98_auth(&auth_header, &ws_url, "GET", None)
                        .await
                    {
                        Ok(user) => return (Some(user), client_id, cm_addr),
                        Err(e) => {
                            warn!("NIP-98 WS auth failed: {}", e);
                        }
                    }
                }
                (None, client_id, cm_addr)
            })
            .map(|(user_opt, client_id, cm_addr), act, ctx| {
                if let Some(user) = user_opt {
                    act.pubkey = Some(user.pubkey.clone());
                    act.is_power_user = user.is_power_user;
                    // codex round-2: identity changed — invalidate any in-flight
                    // (pre-auth) sync so its public-only result cannot land after
                    // the post-auth sync below.
                    act.bump_sync_generation();

                    if let Some(cid) = client_id {
                        use crate::actors::messages::AuthenticateClient;
                        cm_addr.do_send(AuthenticateClient {
                            client_id: cid,
                            pubkey: user.pubkey.clone(),
                            is_power_user: user.is_power_user,
                            ephemeral: false, // NIP-98 is a real identity
                        });
                    }

                    let response = serde_json::json!({
                        "type": "authenticate_success",
                        "pubkey": user.pubkey,
                        "is_power_user": user.is_power_user,
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    });
                    if let Ok(msg_str) = serde_json::to_string(&response) {
                        ctx.text(msg_str);
                    }
                    info!(
                        "NIP-98 WS authenticated: pubkey={}, power_user={}",
                        user.pubkey, user.is_power_user
                    );
                    // #1 (codex): the visibility drop-set in `send_full_state_sync`
                    // snapshots `pubkey` at send time. A client that requested
                    // initial data BEFORE authenticating received the anon
                    // (public-only) view; now that `act.pubkey` is set, re-push a
                    // fresh filtered sync so the owner's private nodes appear
                    // without needing to reconnect.
                    act.send_full_state_sync(ctx);
                } else {
                    let error_msg = serde_json::json!({
                        "type": "error",
                        "message": "NIP-98 WebSocket authentication failed"
                    });
                    if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                        ctx.text(msg_str);
                    }
                    warn!("NIP-98 WS authentication failed for client");
                }
            }),
        );
    } else {
        // --- Legacy path: { type: "authenticate", token, pubkey, ephemeral? } ---
        let token = msg.get("token").and_then(|t| t.as_str()).map(String::from);
        let pubkey = msg.get("pubkey").and_then(|p| p.as_str()).map(String::from);
        let is_ephemeral = msg
            .get("ephemeral")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);

        if let (Some(token), Some(pubkey)) = (token, pubkey) {
            // ADR-06 §D1 parity: the REST extractor accepts the literal
            // dev-session-token in dev builds (auth_extractor.rs), but this WS
            // path only knew real Nostr sessions — so the dev client's
            // authenticate frame was rejected on every connection, spamming an
            // error frame and leaving the WS session without power_user. Same
            // compile-time gate as REST: stripped from release builds unless
            // the dev-auth feature is on.
            #[cfg(any(debug_assertions, feature = "dev-auth"))]
            {
                // Same single gate as REST: only when the handshake marked this
                // connection dev-bypass-eligible (DEV_AUTH_LOOPBACK=1 + loopback
                // peer). Never accept the literal token ungated.
                if token == "dev-session-token" && !act.dev_bypass_ok {
                    warn!(
                        "dev-auth: rejected WS dev-session-token — requires DEV_AUTH_LOOPBACK=1 and a loopback peer"
                    );
                    let response = serde_json::json!({
                        "type": "authenticate_error",
                        "error": "dev-session-token not permitted (requires loopback + DEV_AUTH_LOOPBACK)",
                    });
                    if let Ok(msg_str) = serde_json::to_string(&response) {
                        ctx.text(msg_str);
                    }
                    return;
                }
                if token == "dev-session-token" {
                    act.pubkey = Some(pubkey.clone());
                    act.is_power_user = true;
                    // codex round-2: identity changed — invalidate in-flight sync.
                    act.bump_sync_generation();
                    if let Some(cid) = act.client_id {
                        use crate::actors::messages::AuthenticateClient;
                        act.client_manager_addr.do_send(AuthenticateClient {
                            client_id: cid,
                            pubkey: pubkey.clone(),
                            is_power_user: true,
                            ephemeral: is_ephemeral,
                        });
                    }
                    let response = serde_json::json!({
                        "type": "authenticate_success",
                        "pubkey": pubkey,
                        "is_power_user": true,
                        "ephemeral": is_ephemeral,
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    });
                    if let Ok(msg_str) = serde_json::to_string(&response) {
                        ctx.text(msg_str);
                    }
                    info!(
                        "dev-auth: WS dev-session-token accepted for pubkey {} (dev build)",
                        pubkey
                    );
                    // #1 (codex): re-push a fresh filtered sync now that pubkey is
                    // set (see NIP-98 branch for rationale).
                    act.send_full_state_sync(ctx);
                    return;
                }
            }

            let nostr_service = act.app_state.nostr_service.clone();
            let client_id = act.client_id;
            let cm_addr = act.client_manager_addr.clone();

            ctx.spawn(
                actix::fut::wrap_future::<_, SocketFlowServer>(async move {
                    if let Some(ref ns) = nostr_service {
                        if let Some(user) = ns.get_session(&token).await {
                            if user.pubkey == pubkey {
                                return (Some(user), client_id, cm_addr);
                            }
                        }
                    }
                    (None, client_id, cm_addr)
                })
                .map(move |(user_opt, client_id, cm_addr), act, ctx| {
                    if let Some(user) = user_opt {
                        act.pubkey = Some(user.pubkey.clone());
                        act.is_power_user = user.is_power_user;
                        // codex round-2: identity changed — invalidate in-flight sync.
                        act.bump_sync_generation();

                        if let Some(cid) = client_id {
                            use crate::actors::messages::AuthenticateClient;
                            cm_addr.do_send(AuthenticateClient {
                                client_id: cid,
                                pubkey: user.pubkey.clone(),
                                is_power_user: user.is_power_user,
                                ephemeral: is_ephemeral,
                            });
                        }

                        let response = serde_json::json!({
                            "type": "authenticate_success",
                            "pubkey": user.pubkey,
                            "is_power_user": user.is_power_user,
                            "ephemeral": is_ephemeral,
                            "timestamp": chrono::Utc::now().timestamp_millis()
                        });
                        if let Ok(msg_str) = serde_json::to_string(&response) {
                            ctx.text(msg_str);
                        }
                        info!(
                            "Client authenticated: pubkey={}, power_user={}, ephemeral={}",
                            user.pubkey, user.is_power_user, is_ephemeral
                        );
                        // #1 (codex): re-push a fresh filtered sync now that pubkey
                        // is set (see NIP-98 branch for rationale).
                        act.send_full_state_sync(ctx);
                    } else {
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "message": "Authentication failed: invalid token or pubkey mismatch"
                        });
                        if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                            ctx.text(msg_str);
                        }
                        warn!("Authentication failed for client");
                    }
                }),
            );
        } else {
            let error_msg = serde_json::json!({
                "type": "error",
                "message": "Authentication requires 'event' (NIP-98) or both 'token' and 'pubkey'"
            });
            if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                ctx.text(msg_str);
            }
        }
    }
}

/// Handle "filter_update" message -- per-client node filtering with optional SQLite persistence (ADR-11).
pub(crate) fn handle_filter_update(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    info!("Client sent filter_update message");

    if let Some(client_id) = act.client_id {
        // Check both nested "filter" key and "data" key (client sends in data)
        let filter_data = msg.get("filter").or_else(|| msg.get("data")).unwrap_or(msg);

        use crate::actors::messages::UpdateClientFilter;
        let update = UpdateClientFilter {
            client_id,
            enabled: filter_data
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            quality_threshold: filter_data
                .get("quality_threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.7),
            authority_threshold: filter_data
                .get("authority_threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
            filter_by_quality: filter_data
                .get("filter_by_quality")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            filter_by_authority: filter_data
                .get("filter_by_authority")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            filter_mode: filter_data
                .get("filter_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("or")
                .to_string(),
            max_nodes: filter_data
                .get("max_nodes")
                .and_then(|v| v.as_i64())
                .map(|n| n as i32),
            include_linked_pages: filter_data
                .get("includeLinkedPages")
                .or_else(|| filter_data.get("include_linked_pages"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            server_remove: filter_data
                .get("server_remove")
                .or_else(|| filter_data.get("serverRemove"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };

        info!(
            "Processing filter update: enabled={}, quality_threshold={}, filter_by_quality={}",
            update.enabled, update.quality_threshold, update.filter_by_quality
        );

        let cm_addr = act.client_manager_addr.clone();
        let pubkey = act.pubkey.clone();

        ctx.spawn(
            actix::fut::wrap_future::<_, SocketFlowServer>(async move {
                match cm_addr.send(update.clone()).await {
                    Ok(Ok(())) => (true, pubkey, update),
                    Ok(Err(e)) => {
                        error!("Failed to update client filter: {}", e);
                        (false, pubkey, update)
                    }
                    Err(e) => {
                        error!("Failed to send filter update: {}", e);
                        (false, pubkey, update)
                    }
                }
            })
            .map(|(success, pubkey_opt, update), act, ctx| {
                if success {
                    // Persist the per-user filter to SQLite (ADR-11 §D5) so it
                    // round-trips with GET /api/user/filter. The filter is already
                    // applied in-memory by the ClientCoordinatorActor above; this
                    // writes the authenticated owner's layer via the same
                    // `user_filter` key + task-local scope the REST handler uses.
                    if let Some(pubkey) = pubkey_opt {
                        let repo = act.app_state.sqlite_settings_repository.clone();
                        let user_filter = UserFilter {
                            pubkey: pubkey.clone(),
                            enabled: update.enabled,
                            quality_threshold: update.quality_threshold,
                            authority_threshold: update.authority_threshold,
                            filter_by_quality: update.filter_by_quality,
                            filter_by_authority: update.filter_by_authority,
                            filter_mode: update.filter_mode.clone(),
                            max_nodes: update.max_nodes.map(|n| n as i64),
                            server_remove: update.server_remove,
                        };
                        ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(async move {
                            match serde_json::to_value(&user_filter) {
                                Ok(json) => {
                                    let persisted = CURRENT_OWNER_PUBKEY
                                        .scope(Some(pubkey.clone()), async move {
                                            repo.set_setting(
                                                "user_filter",
                                                SettingValue::Json(json),
                                                Some("Per-user node visibility filter (ADR-11 §D5, WS filter_update)"),
                                            )
                                            .await
                                        })
                                        .await;
                                    match persisted {
                                        Ok(()) => info!(
                                            "Persisted WS filter update for pubkey {} to SQLite",
                                            pubkey
                                        ),
                                        Err(e) => error!(
                                            "Failed to persist WS filter for {}: {}",
                                            pubkey, e
                                        ),
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to serialize WS filter for {}: {}", pubkey, e)
                                }
                            }
                        }));
                    }

                    let response = serde_json::json!({
                        "type": "filter_update_success",
                        "enabled": update.enabled,
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    });
                    if let Ok(msg_str) = serde_json::to_string(&response) {
                        ctx.text(msg_str);
                    }
                } else {
                    let error_msg = serde_json::json!({
                        "type": "error",
                        "message": "Failed to update filter"
                    });
                    if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                        ctx.text(msg_str);
                    }
                }
            }),
        );
    } else {
        warn!("filter_update received but client_id not yet assigned - registration may still be in progress");
        let error_msg = serde_json::json!({
            "type": "error",
            "message": "Client registration in progress, please retry filter update in a moment"
        });
        if let Ok(msg_str) = serde_json::to_string(&error_msg) {
            ctx.text(msg_str);
        }
    }
}

/// Handle ontology validation requests.
pub(crate) fn handle_ontology_validation(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    info!("[WebSocket] Ontology validation request received");
    let ontology_id = msg
        .get("ontologyId")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    if let Some(ref ontology_addr) = act.app_state.ontology_actor_addr {
        let addr = ontology_addr.clone();
        let fut = async move {
            use crate::actors::messages::GetOntologyReport;
            match addr
                .send(GetOntologyReport {
                    report_id: Some(ontology_id.clone()),
                })
                .await
            {
                Ok(Ok(Some(report))) => {
                    serde_json::json!({
                        "type": "ontology_validation_update",
                        "ontologyId": ontology_id,
                        "status": "completed",
                        "violations": report.violations.len(),
                        "inferredTriples": report.inferred_triples.len(),
                        "constraints": report.constraint_summary.total_constraints,
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    })
                }
                Ok(Ok(None)) => {
                    serde_json::json!({
                        "type": "ontology_validation_update",
                        "ontologyId": ontology_id,
                        "status": "not_found",
                        "message": "No validation report available. Run validation first.",
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    })
                }
                _ => {
                    serde_json::json!({
                        "type": "ontology_validation_update",
                        "status": "error",
                        "message": "Failed to retrieve validation report",
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    })
                }
            }
        };

        let fut = actix::fut::wrap_future::<_, SocketFlowServer>(fut);
        ctx.spawn(fut.map(|response, _act, ctx| {
            if let Ok(msg_str) = serde_json::to_string(&response) {
                ctx.text(msg_str);
            }
        }));
    } else {
        let response = serde_json::json!({
            "type": "ontology_validation_update",
            "status": "unavailable",
            "message": "Ontology system not initialized"
        });
        if let Ok(msg_str) = serde_json::to_string(&response) {
            ctx.text(msg_str);
        }
    }
}

/// Handle ontology constraint update/toggle requests.
pub(crate) fn handle_ontology_constraint_update(ctx: &mut <SocketFlowServer as Actor>::Context) {
    info!("[WebSocket] Ontology constraint update request");
    let response = serde_json::json!({
        "type": "ontology_constraint_update",
        "status": "acknowledged",
        "message": "Use REST API /api/ontology-physics/enable for constraint management",
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&response) {
        ctx.text(msg_str);
    }
}

/// Handle ontology reasoning requests.
pub(crate) fn handle_ontology_reasoning(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    info!("[WebSocket] Ontology reasoning request received");
    if let Some(ref ontology_addr) = act.app_state.ontology_actor_addr {
        let addr = ontology_addr.clone();
        let ontology_id = msg.get("ontologyId").and_then(|v| v.as_i64()).unwrap_or(0);
        let source = msg
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("websocket")
            .to_string();

        let fut = async move {
            use crate::actors::ontology_actor::TriggerReasoning;
            match addr
                .send(TriggerReasoning {
                    ontology_id,
                    source,
                })
                .await
            {
                Ok(Ok(job_id)) => serde_json::json!({
                    "type": "ontology_reasoning_started",
                    "jobId": job_id,
                    "timestamp": chrono::Utc::now().timestamp_millis()
                }),
                _ => serde_json::json!({
                    "type": "ontology_reasoning_error",
                    "message": "Failed to trigger reasoning",
                    "timestamp": chrono::Utc::now().timestamp_millis()
                }),
            }
        };
        let fut = actix::fut::wrap_future::<_, SocketFlowServer>(fut);
        ctx.spawn(fut.map(|response, _act, ctx| {
            if let Ok(msg_str) = serde_json::to_string(&response) {
                ctx.text(msg_str);
            }
        }));
    }
}
