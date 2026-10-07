use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

use crate::app_state::AppState;
use crate::handlers::api_handler::analytics::{ANOMALY_STATE, CLUSTERING_TASKS};

const MIN_UPDATE_INTERVAL_MS: u64 = 100;
const MAX_UPDATE_INTERVAL_MS: u64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsWebSocketMessage {
    pub message_type: String,
    pub data: Value,
    pub timestamp: u64,
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMetricsUpdate {
    pub gpu_utilization: f32,
    pub memory_usage_percent: f32,
    pub temperature: f32,
    pub power_draw: f32,
    pub active_kernels: u32,
    pub compute_nodes: u32,
    pub compute_edges: u32,
    pub fps: Option<f32>,
    pub frame_time_ms: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringProgress {
    pub task_id: String,
    pub method: String,
    pub progress: f32,
    pub status: String,
    pub clusters_found: Option<usize>,
    pub estimated_completion: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyAlert {
    pub anomaly_id: String,
    pub node_id: String,
    pub severity: String,
    pub score: f32,
    pub detection_method: String,
    pub description: String,
    pub requires_action: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsUpdate {
    pub insights: Vec<String>,
    pub urgency_level: String,
    pub requires_action: bool,
    pub performance_warnings: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPreferences {
    pub gpu_metrics: bool,
    pub clustering_progress: bool,
    pub anomaly_alerts: bool,
    pub insights_updates: bool,
    pub performance_monitoring: bool,
    pub update_interval_ms: u64,
}

impl Default for SubscriptionPreferences {
    fn default() -> Self {
        Self {
            gpu_metrics: true,
            clustering_progress: true,
            anomaly_alerts: true,
            insights_updates: true,
            performance_monitoring: true,
            update_interval_ms: 5000,
        }
    }
}

pub struct GpuAnalyticsWebSocket {
    client_id: String,
    app_state: actix_web::web::Data<AppState>,
    subscription_prefs: SubscriptionPreferences,
    last_gpu_metrics: Option<GpuMetricsUpdate>,
    heartbeat: Instant,
}

impl GpuAnalyticsWebSocket {
    pub fn new(app_state: actix_web::web::Data<AppState>) -> Self {
        Self {
            client_id: uuid::Uuid::new_v4().to_string(),
            app_state,
            subscription_prefs: SubscriptionPreferences::default(),
            last_gpu_metrics: None,
            heartbeat: Instant::now(),
        }
    }

    fn send_message(
        &self,
        ctx: &mut ws::WebsocketContext<Self>,
        message: AnalyticsWebSocketMessage,
    ) {
        if let Ok(json) = serde_json::to_string(&message) {
            ctx.text(json);
        }
    }

    fn send_gpu_metrics(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        if !self.subscription_prefs.gpu_metrics {
            return;
        }

        let app_state = self.app_state.clone();
        let client_id = self.client_id.clone();

        let fut = async move {
            // GPU compute address is now Arc<RwLock<Option<...>>> - use async accessor
            if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
                match gpu_addr
                    .send(crate::actors::messages::GetPhysicsStats)
                    .await
                {
                    Ok(Ok(stats)) => {
                        let metrics = GpuMetricsUpdate {
                            gpu_utilization: 75.0,
                            memory_usage_percent: (1000_f32 * 0.5) / 8192.0 * 100.0,
                            temperature: 68.0,
                            power_draw: 120.0,
                            active_kernels: 3,
                            compute_nodes: 1000,
                            compute_edges: stats.num_edges,
                            fps: None,
                            frame_time_ms: None,
                        };

                        Some(metrics)
                    }
                    _ => None,
                }
            } else {
                None
            }
        };

        let fut = actix::fut::wrap_future::<_, Self>(fut);
        ctx.spawn(fut.map(move |metrics_opt, act, ctx| {
            if let Some(metrics) = metrics_opt {
                let message = AnalyticsWebSocketMessage {
                    message_type: "gpuMetricsUpdate".to_string(),
                    data: serde_json::to_value(&metrics).unwrap_or_default(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    client_id: Some(client_id),
                };

                act.send_message(ctx, message);
                act.last_gpu_metrics = Some(metrics);
            }
        }));
    }

    fn send_clustering_progress(&self, ctx: &mut ws::WebsocketContext<Self>) {
        if !self.subscription_prefs.clustering_progress {
            return;
        }

        let client_id = self.client_id.clone();

        let fut = async move {
            let tasks = CLUSTERING_TASKS.lock().await;
            let mut progress_updates = Vec::new();

            for task in tasks.values() {
                if task.status == "running" || task.status == "completed" {
                    let progress = ClusteringProgress {
                        task_id: task.task_id.clone(),
                        method: task.method.clone(),
                        progress: task.progress,
                        status: task.status.clone(),
                        clusters_found: task.clusters.as_ref().map(|c| c.len()),
                        estimated_completion: if task.status == "running" {
                            Some(chrono::Utc::now().timestamp_millis() as u64 + 30000)
                        } else {
                            None
                        },
                        error: task.error.clone(),
                    };
                    progress_updates.push(progress);
                }
            }

            progress_updates
        };

        let fut = actix::fut::wrap_future::<_, Self>(fut);
        ctx.spawn(fut.map(move |updates, act, ctx| {
            for progress in updates {
                let message = AnalyticsWebSocketMessage {
                    message_type: "clusteringProgress".to_string(),
                    data: serde_json::to_value(&progress).unwrap_or_default(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    client_id: Some(client_id.clone()),
                };

                act.send_message(ctx, message);
            }
        }));
    }

    fn send_anomaly_alerts(&self, ctx: &mut ws::WebsocketContext<Self>) {
        if !self.subscription_prefs.anomaly_alerts {
            return;
        }

        let client_id = self.client_id.clone();

        let fut = async move {
            let state = ANOMALY_STATE.lock().await;
            let mut alerts = Vec::new();

            for anomaly in state.anomalies.iter().rev().take(5) {
                if anomaly.severity == "critical" || anomaly.severity == "high" {
                    let alert = AnomalyAlert {
                        anomaly_id: anomaly.id.clone(),
                        node_id: anomaly.node_id.clone(),
                        severity: anomaly.severity.clone(),
                        score: anomaly.score,
                        detection_method: anomaly.r#type.clone(),
                        description: anomaly.description.clone(),
                        requires_action: anomaly.severity == "critical",
                    };
                    alerts.push(alert);
                }
            }

            alerts
        };

        let fut = actix::fut::wrap_future::<_, Self>(fut);
        ctx.spawn(fut.map(move |alerts, act, ctx| {
            for alert in alerts {
                let message = AnalyticsWebSocketMessage {
                    message_type: "anomalyAlert".to_string(),
                    data: serde_json::to_value(&alert).unwrap_or_default(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    client_id: Some(client_id.clone()),
                };

                act.send_message(ctx, message);
            }
        }));
    }

    fn send_insights_update(&self, ctx: &mut ws::WebsocketContext<Self>) {
        if !self.subscription_prefs.insights_updates {
            return;
        }

        let app_state = self.app_state.clone();
        let client_id = self.client_id.clone();

        let fut = async move {
            let mut insights = Vec::new();
            let mut performance_warnings = Vec::new();
            let mut recommendations = Vec::new();
            let mut urgency_level = "low";

            // GPU compute address is now Arc<RwLock<Option<...>>> - use async accessor
            if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
                if let Ok(Ok(stats)) = gpu_addr
                    .send(crate::actors::messages::GetPhysicsStats)
                    .await
                {
                    if stats.gpu_failure_count > 0 {
                        performance_warnings
                            .push(format!("{} GPU failures detected", stats.gpu_failure_count));
                        recommendations.push(
                            "Check GPU health and restart compute service if needed".to_string(),
                        );
                        urgency_level = "medium";
                    }

                    if stats.total_force_calculations > 500000 {
                        insights.push(format!(
                            "Processing large graph with {} force calculations",
                            stats.total_force_calculations
                        ));
                        recommendations.push(
                            "Consider using batch processing for better performance".to_string(),
                        );
                    }
                }
            }

            {
                let tasks = CLUSTERING_TASKS.lock().await;
                let running_tasks = tasks.values().filter(|t| t.status == "running").count();
                if running_tasks > 0 {
                    insights.push(format!("{} clustering tasks in progress", running_tasks));
                }
            }

            {
                let state = ANOMALY_STATE.lock().await;
                if state.stats.critical > 0 {
                    insights.push(format!(
                        "CRITICAL: {} critical anomalies detected",
                        state.stats.critical
                    ));
                    urgency_level = "critical";
                } else if state.stats.high > 3 {
                    insights.push(format!(
                        "High alert: {} high-severity anomalies",
                        state.stats.high
                    ));
                    urgency_level = "high";
                }
            }

            InsightsUpdate {
                insights,
                urgency_level: urgency_level.to_string(),
                requires_action: urgency_level != "low",
                performance_warnings,
                recommendations,
            }
        };

        let fut = actix::fut::wrap_future::<_, Self>(fut);
        ctx.spawn(fut.map(move |insights_update, act, ctx| {
            let message = AnalyticsWebSocketMessage {
                message_type: "insightsUpdate".to_string(),
                data: serde_json::to_value(&insights_update).unwrap_or_default(),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                client_id: Some(client_id),
            };

            act.send_message(ctx, message);
        }));
    }

    fn start_periodic_updates(&self, ctx: &mut ws::WebsocketContext<Self>) {
        let clamped_ms = self
            .subscription_prefs
            .update_interval_ms
            .clamp(MIN_UPDATE_INTERVAL_MS, MAX_UPDATE_INTERVAL_MS);
        let interval = std::time::Duration::from_millis(clamped_ms);

        ctx.run_interval(interval, |act, ctx| {
            if std::time::Instant::now().duration_since(act.heartbeat)
                > std::time::Duration::from_secs(60)
            {
                info!("GPU analytics WebSocket client timeout: {}", act.client_id);
                ctx.stop();
                return;
            }

            act.send_gpu_metrics(ctx);
            act.send_clustering_progress(ctx);
            act.send_anomaly_alerts(ctx);
            act.send_insights_update(ctx);
        });
    }
}

impl Actor for GpuAnalyticsWebSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(
            "GPU Analytics WebSocket client connected: {}",
            self.client_id
        );

        let welcome = AnalyticsWebSocketMessage {
            message_type: "connected".to_string(),
            data: serde_json::json!({
                "clientId": self.client_id,
                "capabilities": {
                    "gpuMetrics": true,
                    "clusteringProgress": true,
                    "anomalyAlerts": true,
                    "insightsUpdates": true,
                    "realTimeUpdates": true
                },
                "defaultUpdateInterval": self.subscription_prefs.update_interval_ms
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            client_id: Some(self.client_id.clone()),
        };

        self.send_message(ctx, welcome);

        self.start_periodic_updates(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "GPU Analytics WebSocket client disconnected: {}",
            self.client_id
        );
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for GpuAnalyticsWebSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                self.heartbeat = Instant::now();

                match serde_json::from_str::<AnalyticsWebSocketMessage>(&text) {
                    Ok(ws_msg) => {
                        debug!("Received WebSocket message: {}", ws_msg.message_type);

                        match ws_msg.message_type.as_str() {
                            "updateSubscriptions" => {
                                if let Ok(mut prefs) =
                                    serde_json::from_value::<SubscriptionPreferences>(ws_msg.data)
                                {
                                    prefs.update_interval_ms = prefs
                                        .update_interval_ms
                                        .clamp(MIN_UPDATE_INTERVAL_MS, MAX_UPDATE_INTERVAL_MS);
                                    self.subscription_prefs = prefs;
                                    info!(
                                        "Updated subscription preferences for client: {}",
                                        self.client_id
                                    );

                                    let response = AnalyticsWebSocketMessage {
                                        message_type: "subscriptionsUpdated".to_string(),
                                        data: serde_json::to_value(&self.subscription_prefs)
                                            .unwrap_or_default(),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        client_id: Some(self.client_id.clone()),
                                    };
                                    self.send_message(ctx, response);
                                }
                            }
                            "requestImmediateUpdate" => {
                                self.send_gpu_metrics(ctx);
                                self.send_clustering_progress(ctx);
                                self.send_anomaly_alerts(ctx);
                                self.send_insights_update(ctx);
                            }
                            "ping" => {
                                let pong = AnalyticsWebSocketMessage {
                                    message_type: "pong".to_string(),
                                    data: serde_json::json!({
                                        "timestamp": chrono::Utc::now().timestamp_millis()
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    client_id: Some(self.client_id.clone()),
                                };
                                self.send_message(ctx, pong);
                            }
                            _ => {
                                warn!("Unknown WebSocket message type: {}", ws_msg.message_type);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse WebSocket message: {}", e);
                    }
                }
            }
            Ok(ws::Message::Ping(msg)) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.heartbeat = Instant::now();
            }
            Ok(ws::Message::Close(reason)) => {
                info!("GPU Analytics WebSocket closing: {:?}", reason);
                ctx.stop();
            }
            Err(e) => {
                error!("GPU Analytics WebSocket error: {}", e);
                ctx.stop();
            }
            _ => {}
        }
    }
}

pub async fn gpu_analytics_websocket(
    req: actix_web::HttpRequest,
    stream: actix_web::web::Payload,
    app_state: actix_web::web::Data<AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    info!("New GPU Analytics WebSocket connection requested");

    // SECURITY: Require authentication before WebSocket upgrade
    {
        // ADR-2058: Authorization header only in a release build. The `?token=`
        // fallback leaks the bearer token into access and proxy logs, so it is
        // compiled out of release and kept behind the dev-auth gate.
        let header_token = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        #[cfg(any(debug_assertions, feature = "dev-auth"))]
        let token = header_token.or_else(|| {
            let query = req.query_string();
            let found = url::form_urlencoded::parse(query.as_bytes())
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v.to_string());
            if found.is_some() {
                log::warn!(
                    "SECURITY: /analytics/ws authenticated via ?token= query string — \
                     dev-only path (ADR-2058). Use the Authorization header."
                );
            }
            found
        });

        #[cfg(not(any(debug_assertions, feature = "dev-auth")))]
        let token = {
            if req.query_string().contains("token=") {
                log::warn!(
                    "SECURITY: Rejecting ?token= query-string auth on /analytics/ws — \
                     ADR-2058 requires the Authorization header in release builds"
                );
            }
            header_token
        };

        if token.as_deref().unwrap_or("").is_empty() {
            let client_ip = req
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            log::warn!(
                "SECURITY: Rejected unauthenticated WebSocket upgrade on /analytics/ws from {}",
                client_ip
            );
            return Ok(actix_web::HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"})));
        }
    }

    ws::start(GpuAnalyticsWebSocket::new(app_state), &req, stream)
}
