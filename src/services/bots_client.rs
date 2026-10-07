use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use crate::actors::messages::UpdateBotsGraph;
use crate::services::agent_visualization_protocol::{McpServerType, MultiMcpAgentStatus};
use crate::utils::mcp_connection::call_agent_spawn;
use crate::utils::mcp_tcp_client::{create_mcp_client, McpTcpClient};
use crate::utils::time;
use actix::Addr;
use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub status: String,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub z: f32,
    #[serde(default = "default_cpu_usage")]
    pub cpu_usage: f32,
    #[serde(default = "default_health")]
    pub health: f32,
    #[serde(default = "default_workload")]
    pub workload: f32,
    #[serde(default = "default_memory_usage")]
    pub memory_usage: f32,
    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<u64>,
    /// Sovereign identity (`did:nostr:<hex>`), minted by agentbox at spawn and
    /// carried here to every client surface (COM-14, ADR-125). `None` until
    /// agentbox attaches it. Populated only after a `uri::did_nostr()` round-trip
    /// (see [`validate_did_nostr`]); the trust key that supersedes `id`
    /// (`task_id`) once verified (WP-1, DDD invariant 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_nostr: Option<String>,
}

/// Accept a claimed `did:nostr` only if it round-trips through the canonical
/// `uri::did_nostr()` primitive (ADR-125 I1): a `ParsedUri::DidNostr` whose
/// re-minted form equals the claim. A malformed or non-DID claim is dropped to
/// `None` rather than carried onto a trusted surface (WP-1, invariant 2). This
/// is the well-formedness gate at carry time; control of the key is proven
/// separately by the Schnorr challenge in `nostr_identity_verifier`.
pub(crate) fn validate_did_nostr(claimed: &str) -> Option<String> {
    match crate::uri::parse(claimed) {
        Ok(crate::uri::ParsedUri::DidNostr { pubkey }) => crate::uri::did_nostr(&pubkey)
            .ok()
            .filter(|minted| minted == claimed),
        _ => {
            warn!("dropping malformed did:nostr claim on agent record: {claimed}");
            None
        }
    }
}

fn default_cpu_usage() -> f32 {
    50.0
}
fn default_health() -> f32 {
    90.0
}
fn default_workload() -> f32 {
    0.7
}
fn default_memory_usage() -> f32 {
    30.0
}

impl From<MultiMcpAgentStatus> for Agent {
    fn from(mcp_agent: MultiMcpAgentStatus) -> Self {
        Agent {
            id: mcp_agent.agent_id,
            name: mcp_agent.name,
            agent_type: mcp_agent.agent_type,
            status: mcp_agent.status,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            cpu_usage: mcp_agent.performance.cpu_usage,
            health: mcp_agent.performance.health_score,
            workload: mcp_agent.performance.activity_level / 100.0,
            memory_usage: mcp_agent.performance.memory_usage,
            created_at: Some(
                chrono::DateTime::from_timestamp(mcp_agent.created_at, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
            ),
            age: Some((time::timestamp_seconds() - mcp_agent.created_at) as u64 * 1000),
            // Carry the agentbox-minted DID onto the trusted Agent form, gated on
            // a uri::did_nostr() round-trip (WP-1). Absent/malformed → None.
            did_nostr: mcp_agent.did_nostr.as_deref().and_then(validate_did_nostr),
        }
    }
}

#[derive(Clone)]
pub struct BotsClient {
    mcp_client: McpTcpClient,
    graph_service_addr: Option<Addr<GraphServiceSupervisor>>,
    agents: Arc<RwLock<Vec<Agent>>>,
    /// Tracks the real MCP connection state (ADR-2088) — set true only after a
    /// successful `test_connection()`, cleared on any subsequent failure so
    /// `get_status()` never reports a constant.
    connected: Arc<AtomicBool>,
}

impl BotsClient {
    pub fn new() -> Self {
        let host = std::env::var("CLAUDE_FLOW_HOST")
            .or_else(|_| std::env::var("MCP_HOST"))
            .unwrap_or_else(|_| "multi-agent-container".to_string());
        let port = std::env::var("MCP_TCP_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(9500);

        let mcp_client = create_mcp_client(&McpServerType::ClaudeFlow, &host, port);

        Self {
            mcp_client,
            graph_service_addr: None,
            agents: Arc::new(RwLock::new(Vec::new())),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_graph_service(graph_addr: Addr<GraphServiceSupervisor>) -> Self {
        let mut client = Self::new();
        client.graph_service_addr = Some(graph_addr);
        client
    }

    pub async fn connect(&self, _bots_url: &str) -> Result<()> {
        info!(
            "Initializing MCP connection to {}:{}",
            self.mcp_client.host, self.mcp_client.port
        );

        match self.mcp_client.test_connection().await {
            Ok(true) => {
                info!("MCP server is reachable");
                self.connected.store(true, Ordering::SeqCst);

                match self.mcp_client.initialize_session().await {
                    Ok(_) => {
                        info!("MCP session initialized successfully");
                    }
                    Err(e) => {
                        warn!("Failed to initialize MCP session: {}", e);
                    }
                }
            }
            Ok(false) => {
                warn!("MCP server is not reachable");
                self.connected.store(false, Ordering::SeqCst);
                return Err(anyhow::anyhow!("MCP server is not reachable"));
            }
            Err(e) => {
                error!("Failed to test MCP connection: {}", e);
                self.connected.store(false, Ordering::SeqCst);
                return Err(anyhow::anyhow!("Failed to test MCP connection: {}", e));
            }
        }

        self.start_polling().await;

        Ok(())
    }

    async fn start_polling(&self) {
        let mcp_client = self.mcp_client.clone();
        let graph_service_addr = self.graph_service_addr.clone();
        let agents = self.agents.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

            loop {
                interval.tick().await;

                match mcp_client.query_agent_list().await {
                    Ok(mcp_agents) => {
                        if !mcp_agents.is_empty() {
                            info!("Received {} agents from MCP server", mcp_agents.len());

                            let converted_agents: Vec<Agent> =
                                mcp_agents.into_iter().map(Agent::from).collect();

                            {
                                let mut agents_lock = agents.write().await;
                                *agents_lock = converted_agents.clone();
                            }

                            if let Some(ref graph_addr) = graph_service_addr {
                                info!(
                                    "📨 BotsClient sending {} agents to graph",
                                    converted_agents.len()
                                );

                                graph_addr.do_send(UpdateBotsGraph {
                                    agents: converted_agents.clone(),
                                });
                            }
                        } else {
                            let mut agents_lock = agents.write().await;
                            if !agents_lock.is_empty() {
                                debug!("Clearing stored agents - MCP returned empty list");
                                agents_lock.clear();
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Failed to query agents from MCP: {}", e);
                    }
                }
            }
        });
    }

    pub async fn get_agents_snapshot(&self) -> Result<Vec<Agent>> {
        let agents = self.agents.read().await;
        Ok(agents.clone())
    }

    pub async fn get_status(&self) -> Result<serde_json::Value> {
        let connected = self.connected.load(Ordering::SeqCst);
        let agents = self.agents.read().await;

        Ok(serde_json::json!({
            "connected": connected,
            "host": self.mcp_client.host,
            "port": self.mcp_client.port,
            "agent_count": agents.len(),
            "agents": agents.iter().map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "name": a.name,
                    "type": a.agent_type,
                    "status": a.status
                })
            }).collect::<Vec<_>>()
        }))
    }

    pub async fn test_connection(&self) -> Result<bool> {
        match self.mcp_client.test_connection().await {
            Ok(result) => Ok(result),
            Err(e) => Err(anyhow::anyhow!("Connection test failed: {}", e)),
        }
    }

    pub async fn spawn_agent_mcp(&self, agent_type: &str, swarm_id: &str) -> Result<String> {
        info!(
            "Spawning MCP agent: type={}, swarm={}",
            agent_type, swarm_id
        );

        let port_str = self.mcp_client.port.to_string();
        match call_agent_spawn(&self.mcp_client.host, &port_str, agent_type, swarm_id).await {
            Ok(response) => {
                let agent_id = if let Some(content) = response.get("content") {
                    if let Some(agent_data) = content.get("agent_id") {
                        agent_data.as_str().unwrap_or("unknown").to_string()
                    } else if let Some(result) = content.get("result") {
                        result.as_str().unwrap_or("mcp_agent").to_string()
                    } else {
                        format!("mcp_{}_{}", agent_type, swarm_id)
                    }
                } else {
                    format!("mcp_{}_{}", agent_type, swarm_id)
                };

                info!(
                    "Successfully spawned MCP agent {} of type {}",
                    agent_id, agent_type
                );
                Ok(agent_id)
            }
            Err(e) => {
                error!("Failed to spawn MCP agent: {}", e);
                Err(anyhow::anyhow!("MCP agent spawn failed: {}", e))
            }
        }
    }
}

impl Default for BotsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-2088: `get_status()` must report the client's resolved host/port,
    /// never the old hardcoded "agentic-workstation":9090 literal.
    #[tokio::test]
    async fn get_status_reports_resolved_host_and_port_not_literals() {
        let client = BotsClient::new();
        let status = client
            .get_status()
            .await
            .expect("get_status should not fail");

        assert_eq!(status["host"], serde_json::json!(client.mcp_client.host));
        assert_eq!(status["port"], serde_json::json!(client.mcp_client.port));
        assert_ne!(
            status["host"],
            serde_json::json!("agentic-workstation"),
            "get_status must not report the old hardcoded host literal"
        );
        assert_ne!(
            status["port"],
            serde_json::json!(9090u16),
            "get_status must not report the old hardcoded port literal"
        );
    }

    /// ADR-2088: `connected` must reflect tracked connection state, never a
    /// constant `true`. A freshly constructed client (connect() not yet run)
    /// must report false; flipping the tracked flag must change the report.
    #[tokio::test]
    async fn get_status_reflects_actual_connection_state_not_a_constant() {
        let client = BotsClient::new();

        let status = client
            .get_status()
            .await
            .expect("get_status should not fail");
        assert_eq!(status["connected"], serde_json::json!(false));

        client.connected.store(true, Ordering::SeqCst);
        let status = client
            .get_status()
            .await
            .expect("get_status should not fail");
        assert_eq!(status["connected"], serde_json::json!(true));
    }
}
