//! Multi-MCP Agent Discovery Service
//!
//! This service discovers and monitors agents across multiple MCP servers:
//! - Claude Flow (claude-flow MCP server)
//! - RuvSwarm (ruv-swarm MCP server)
//! - DAA (Decentralized Autonomous Agents)
//! - Custom MCP implementations
//!
//! It provides unified agent discovery, real-time monitoring, and topology analysis.

use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::services::agent_visualization_protocol::{
    GlobalPerformanceMetrics, McpServerInfo, McpServerType, MultiMcpAgentStatus, SwarmTopologyData,
};
use crate::utils::mcp_tcp_client::create_mcp_client;
use crate::utils::time;

/// ADR-2083: floor for the discovery loop's sleep interval. Each
/// `McpServerConfig::discovery_interval_ms` is operator/deploy-time
/// configuration; a misconfigured value of `0` (or anything absurdly small)
/// must not spin the discovery loop into a busy-poll. Every derived interval
/// is clamped to at least this many milliseconds regardless of what the
/// configured servers ask for.
const MIN_DISCOVERY_INTERVAL_MS: u64 = 250;

/// ADR-2083: fallback cadence used when there is no enabled server to derive
/// an interval from (e.g. every server has been removed or disabled).
/// Matches `McpServerConfig::default()`'s `discovery_interval_ms` so the
/// no-server case behaves the same as a single default-configured server.
const DEFAULT_DISCOVERY_INTERVAL_MS: u64 = 5000;

/// ADR-2083: selects the multi-MCP discovery loop's sleep interval from the
/// configured per-server `discovery_interval_ms` values.
///
/// Semantics: the minimum `discovery_interval_ms` across *enabled* servers,
/// so every server is polled at least as often as it asked to be, floored at
/// [`MIN_DISCOVERY_INTERVAL_MS`] so a misconfigured `0` cannot spin the loop.
/// Disabled servers never influence the result. Falls back to
/// [`DEFAULT_DISCOVERY_INTERVAL_MS`] when no server is enabled.
fn select_discovery_interval_ms<'a, I>(configs: I) -> u64
where
    I: IntoIterator<Item = &'a McpServerConfig>,
{
    let min_enabled = configs
        .into_iter()
        .filter(|config| config.enabled)
        .map(|config| config.discovery_interval_ms)
        .min();

    match min_enabled {
        Some(interval_ms) => interval_ms.max(MIN_DISCOVERY_INTERVAL_MS),
        None => DEFAULT_DISCOVERY_INTERVAL_MS,
    }
}

#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub server_id: String,
    pub server_type: McpServerType,
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    pub discovery_interval_ms: u64,
    pub timeout_ms: u64,
    pub retry_attempts: u32,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            server_id: "unknown".to_string(),
            server_type: McpServerType::Custom("unknown".to_string()),
            host: "localhost".to_string(),
            port: 9500,
            enabled: true,
            discovery_interval_ms: 5000,
            timeout_ms: 10000,
            retry_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
    pub total_discoveries: u64,
    pub successful_discoveries: u64,
    pub failed_discoveries: u64,
    pub total_agents_discovered: u64,
    pub last_discovery_time: Option<DateTime<Utc>>,
    pub average_discovery_time_ms: f64,
    pub servers_online: u32,
    pub servers_offline: u32,
}

pub struct MultiMcpAgentDiscovery {
    servers: Arc<RwLock<HashMap<String, McpServerConfig>>>,
    discovered_agents: Arc<RwLock<HashMap<String, MultiMcpAgentStatus>>>,
    server_statuses: Arc<RwLock<HashMap<String, McpServerInfo>>>,
    topology_data: Arc<RwLock<HashMap<String, SwarmTopologyData>>>,
    stats: Arc<RwLock<DiscoveryStats>>,
    discovery_running: Arc<RwLock<bool>>,
}

impl MultiMcpAgentDiscovery {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            discovered_agents: Arc::new(RwLock::new(HashMap::new())),
            server_statuses: Arc::new(RwLock::new(HashMap::new())),
            topology_data: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(DiscoveryStats::default())),
            discovery_running: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn initialize_default_servers(&self) {
        let mut servers = self.servers.write().await;

        servers.insert(
            "claude-flow".to_string(),
            McpServerConfig {
                server_id: "claude-flow".to_string(),
                server_type: McpServerType::ClaudeFlow,
                host: std::env::var("CLAUDE_FLOW_HOST").unwrap_or_else(|_| "localhost".to_string()),
                port: std::env::var("MCP_TCP_PORT")
                    .unwrap_or_else(|_| "9500".to_string())
                    .parse()
                    .unwrap_or(9500),
                enabled: true,
                discovery_interval_ms: 3000,
                timeout_ms: 10000,
                retry_attempts: 3,
            },
        );

        servers.insert(
            "ruv-swarm".to_string(),
            McpServerConfig {
                server_id: "ruv-swarm".to_string(),
                server_type: McpServerType::RuvSwarm,
                host: std::env::var("RUV_SWARM_HOST").unwrap_or_else(|_| "localhost".to_string()),
                port: std::env::var("RUV_SWARM_PORT")
                    .unwrap_or_else(|_| "9501".to_string())
                    .parse()
                    .unwrap_or(9501),
                enabled: true,
                discovery_interval_ms: 3000,
                timeout_ms: 10000,
                retry_attempts: 3,
            },
        );

        servers.insert(
            "daa".to_string(),
            McpServerConfig {
                server_id: "daa".to_string(),
                server_type: McpServerType::Daa,
                host: std::env::var("DAA_HOST").unwrap_or_else(|_| "localhost".to_string()),
                port: std::env::var("DAA_PORT")
                    .unwrap_or_else(|_| "9502".to_string())
                    .parse()
                    .unwrap_or(9502),
                enabled: true,
                discovery_interval_ms: 5000,
                timeout_ms: 15000,
                retry_attempts: 2,
            },
        );

        info!(
            "Initialized {} default MCP servers for discovery",
            servers.len()
        );
    }

    pub async fn add_server(&self, config: McpServerConfig) {
        let mut servers = self.servers.write().await;
        info!(
            "Adding MCP server: {} ({}:{})",
            config.server_id, config.host, config.port
        );
        servers.insert(config.server_id.clone(), config);
    }

    pub async fn remove_server(&self, server_id: &str) {
        let mut servers = self.servers.write().await;
        if servers.remove(server_id).is_some() {
            info!("Removed MCP server: {}", server_id);

            let mut agents = self.discovered_agents.write().await;
            agents.retain(|_, agent| {
                !matches!(
                    (&agent.server_source, server_id),
                    (McpServerType::ClaudeFlow, "claude-flow")
                        | (McpServerType::RuvSwarm, "ruv-swarm")
                        | (McpServerType::Daa, "daa")
                )
            });
        }
    }

    pub async fn start_discovery(&self) {
        let mut discovery_running = self.discovery_running.write().await;
        if *discovery_running {
            warn!("Discovery already running");
            return;
        }
        *discovery_running = true;
        drop(discovery_running);

        info!("Starting multi-MCP agent discovery service");

        let servers = self.servers.clone();
        let discovered_agents = self.discovered_agents.clone();
        let server_statuses = self.server_statuses.clone();
        let topology_data = self.topology_data.clone();
        let stats = self.stats.clone();
        let discovery_running = self.discovery_running.clone();

        tokio::spawn(async move {
            while *discovery_running.read().await {
                let servers_config = servers.read().await.clone();

                // ADR-2083: derive the inter-cycle sleep from the configured
                // per-server discovery_interval_ms BEFORE servers_config is
                // consumed by the discovery_futures map below.
                let sleep_ms = select_discovery_interval_ms(servers_config.values());

                // Discover all enabled servers concurrently
                let discovery_futures: Vec<_> = servers_config
                    .into_iter()
                    .filter(|(_, config)| config.enabled)
                    .map(|(server_id, config)| {
                        let discovered = Arc::clone(&discovered_agents);
                        let statuses = Arc::clone(&server_statuses);
                        let topo_data = Arc::clone(&topology_data);
                        let stats_ref = Arc::clone(&stats);
                        async move {
                            let start_time = std::time::Instant::now();

                            match Self::discover_server_agents(&config).await {
                                Ok((server_info, agents, topology)) => {
                                    {
                                        let mut guard = statuses.write().await;
                                        guard.insert(server_id.clone(), server_info);
                                    }
                                    {
                                        let mut guard = discovered.write().await;
                                        for agent in agents {
                                            guard.insert(agent.agent_id.clone(), agent);
                                        }
                                    }
                                    if let Some(topo) = topology {
                                        let mut guard = topo_data.write().await;
                                        guard.insert(server_id.clone(), topo);
                                    }

                                    let discovery_time = start_time.elapsed().as_millis() as f64;
                                    {
                                        let mut guard = stats_ref.write().await;
                                        guard.successful_discoveries += 1;
                                        guard.last_discovery_time = Some(time::now());
                                        guard.average_discovery_time_ms =
                                            (guard.average_discovery_time_ms + discovery_time)
                                                / 2.0;
                                    }

                                    debug!(
                                        "Successfully discovered agents from {} in {}ms",
                                        server_id, discovery_time
                                    );
                                }
                                Err(e) => {
                                    error!("Failed to discover agents from {}: {}", server_id, e);
                                    {
                                        let mut guard = stats_ref.write().await;
                                        guard.failed_discoveries += 1;
                                    }
                                    {
                                        let mut guard = statuses.write().await;
                                        if let Some(server_info) = guard.get_mut(&server_id) {
                                            server_info.is_connected = false;
                                            server_info.last_heartbeat = time::timestamp_seconds();
                                        }
                                    }
                                }
                            }
                        }
                    })
                    .collect();

                futures::future::join_all(discovery_futures).await;

                // ADR-2083: sleep for the configured discovery cadence rather
                // than a hardcoded 1000ms — honours each server's
                // discovery_interval_ms (minimum across enabled servers,
                // floored at MIN_DISCOVERY_INTERVAL_MS).
                tokio::time::sleep(tokio::time::Duration::from_millis(sleep_ms)).await;
            }

            info!("Multi-MCP agent discovery service stopped");
        });
    }

    pub async fn stop_discovery(&self) {
        let mut discovery_running = self.discovery_running.write().await;
        *discovery_running = false;
        info!("Stopping multi-MCP agent discovery service");
    }

    async fn discover_server_agents(
        config: &McpServerConfig,
    ) -> Result<
        (
            McpServerInfo,
            Vec<MultiMcpAgentStatus>,
            Option<SwarmTopologyData>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let start_time = time::now();

        let _discovery_duration = start_time;

        debug!(
            "Discovering agents from {} server at {}:{}",
            config.server_id, config.host, config.port
        );

        match &config.server_type {
            McpServerType::ClaudeFlow => Self::discover_claude_flow_agents(config).await,
            McpServerType::RuvSwarm => Self::discover_ruv_swarm_agents(config).await,
            McpServerType::Daa => Self::discover_daa_agents(config).await,
            McpServerType::Custom(name) => {
                warn!("Custom MCP server type '{}' not implemented", name);
                Err("Custom server type not implemented".into())
            }
        }
    }

    async fn discover_claude_flow_agents(
        config: &McpServerConfig,
    ) -> Result<
        (
            McpServerInfo,
            Vec<MultiMcpAgentStatus>,
            Option<SwarmTopologyData>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        debug!(
            "Discovering Claude Flow agents from real MCP server at {}:{}",
            config.host, config.port
        );

        let client = create_mcp_client(&config.server_type, &config.host, config.port);

        let is_connected = client.test_connection().await.unwrap_or(false);
        if !is_connected {
            return Err(format!(
                "Cannot connect to Claude Flow MCP server at {}:{}",
                config.host, config.port
            )
            .into());
        }

        if let Err(e) = client.initialize_session().await {
            warn!("Failed to initialize MCP session: {}", e);
        }

        let mut server_info = match client.query_server_info().await {
            Ok(info) => info,
            Err(e) => {
                warn!("Failed to get server info, using defaults: {}", e);
                McpServerInfo {
                    server_id: config.server_id.clone(),
                    server_type: config.server_type.clone(),
                    host: config.host.clone(),
                    port: config.port,
                    is_connected,
                    last_heartbeat: time::timestamp_seconds(),
                    supported_tools: vec![
                        "agent_list".to_string(),
                        "swarm_status".to_string(),
                        "server_info".to_string(),
                    ],
                    agent_count: 0,
                }
            }
        };

        let agents = match client.query_agent_list().await {
            Ok(agent_list) => {
                info!(
                    "Retrieved {} agents from Claude Flow MCP server",
                    agent_list.len()
                );

                agent_list
                    .into_iter()
                    .map(|mut agent| {
                        agent.server_source = McpServerType::ClaudeFlow;
                        agent
                    })
                    .collect()
            }
            Err(e) => {
                error!("Failed to query agent list from Claude Flow MCP: {}", e);
                Vec::new()
            }
        };

        server_info.agent_count = agents.len() as u32;

        let topology = match client.query_swarm_status().await {
            Ok(topology_data) => {
                info!("Retrieved topology data from Claude Flow MCP server");
                Some(topology_data)
            }
            Err(e) => {
                warn!("Failed to query swarm topology from Claude Flow MCP: {}", e);
                None
            }
        };

        Ok((server_info, agents, topology))
    }

    async fn discover_ruv_swarm_agents(
        config: &McpServerConfig,
    ) -> Result<
        (
            McpServerInfo,
            Vec<MultiMcpAgentStatus>,
            Option<SwarmTopologyData>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        debug!(
            "Discovering RuvSwarm agents from real MCP server at {}:{}",
            config.host, config.port
        );

        let client = create_mcp_client(&config.server_type, &config.host, config.port);

        let is_connected = client.test_connection().await.unwrap_or(false);
        if !is_connected {
            return Err(format!(
                "Cannot connect to RuvSwarm MCP server at {}:{}",
                config.host, config.port
            )
            .into());
        }

        if let Err(e) = client.initialize_session().await {
            warn!("Failed to initialize MCP session: {}", e);
        }

        let mut server_info = match client.query_server_info().await {
            Ok(mut info) => {
                info.server_type = McpServerType::RuvSwarm;
                info
            }
            Err(e) => {
                warn!("Failed to get server info, using defaults: {}", e);
                McpServerInfo {
                    server_id: config.server_id.clone(),
                    server_type: config.server_type.clone(),
                    host: config.host.clone(),
                    port: config.port,
                    is_connected,
                    last_heartbeat: time::timestamp_seconds(),
                    supported_tools: vec![
                        "swarm_init".to_string(),
                        "agent_spawn".to_string(),
                        "daa_init".to_string(),
                        "neural_train".to_string(),
                        "benchmark_run".to_string(),
                    ],
                    agent_count: 0,
                }
            }
        };

        let agents = match client.query_agent_list().await {
            Ok(agent_list) => {
                info!(
                    "Retrieved {} agents from RuvSwarm MCP server",
                    agent_list.len()
                );

                agent_list
                    .into_iter()
                    .map(|mut agent| {
                        agent.server_source = McpServerType::RuvSwarm;
                        agent
                    })
                    .collect()
            }
            Err(e) => {
                error!("Failed to query agent list from RuvSwarm MCP: {}", e);
                Vec::new()
            }
        };

        server_info.agent_count = agents.len() as u32;

        let topology = match client.query_swarm_status().await {
            Ok(topology_data) => {
                info!("Retrieved topology data from RuvSwarm MCP server");
                Some(topology_data)
            }
            Err(e) => {
                warn!("Failed to query swarm topology from RuvSwarm MCP: {}", e);
                None
            }
        };

        Ok((server_info, agents, topology))
    }

    async fn discover_daa_agents(
        config: &McpServerConfig,
    ) -> Result<
        (
            McpServerInfo,
            Vec<MultiMcpAgentStatus>,
            Option<SwarmTopologyData>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        debug!(
            "Discovering DAA agents from real MCP server at {}:{}",
            config.host, config.port
        );

        let client = create_mcp_client(&config.server_type, &config.host, config.port);

        let is_connected = client.test_connection().await.unwrap_or(false);
        if !is_connected {
            return Err(format!(
                "Cannot connect to DAA MCP server at {}:{}",
                config.host, config.port
            )
            .into());
        }

        if let Err(e) = client.initialize_session().await {
            warn!("Failed to initialize MCP session: {}", e);
        }

        let mut server_info = match client.query_server_info().await {
            Ok(mut info) => {
                info.server_type = McpServerType::Daa;
                info
            }
            Err(e) => {
                warn!("Failed to get server info, using defaults: {}", e);
                McpServerInfo {
                    server_id: config.server_id.clone(),
                    server_type: config.server_type.clone(),
                    host: config.host.clone(),
                    port: config.port,
                    is_connected,
                    last_heartbeat: time::timestamp_seconds(),
                    supported_tools: vec![
                        "daa_agent_create".to_string(),
                        "daa_workflow_create".to_string(),
                        "daa_knowledge_share".to_string(),
                        "daa_learning_status".to_string(),
                    ],
                    agent_count: 0,
                }
            }
        };

        let agents = match client.query_agent_list().await {
            Ok(agent_list) => {
                info!("Retrieved {} agents from DAA MCP server", agent_list.len());

                agent_list
                    .into_iter()
                    .map(|mut agent| {
                        agent.server_source = McpServerType::Daa;
                        agent
                    })
                    .collect()
            }
            Err(e) => {
                error!("Failed to query agent list from DAA MCP: {}", e);
                Vec::new()
            }
        };

        server_info.agent_count = agents.len() as u32;

        let topology = match client.query_swarm_status().await {
            Ok(topology_data) => {
                info!("Retrieved topology data from DAA MCP server");
                Some(topology_data)
            }
            Err(e) => {
                warn!("Failed to query swarm topology from DAA MCP: {}", e);
                None
            }
        };

        Ok((server_info, agents, topology))
    }

    pub async fn get_all_agents(&self) -> Vec<MultiMcpAgentStatus> {
        self.discovered_agents
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    pub async fn get_agents_by_server(
        &self,
        server_type: &McpServerType,
    ) -> Vec<MultiMcpAgentStatus> {
        self.discovered_agents
            .read()
            .await
            .values()
            .filter(|agent| {
                std::mem::discriminant(&agent.server_source) == std::mem::discriminant(server_type)
            })
            .cloned()
            .collect()
    }

    pub async fn get_server_statuses(&self) -> Vec<McpServerInfo> {
        self.server_statuses
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    pub async fn get_discovery_stats(&self) -> DiscoveryStats {
        self.stats.read().await.clone()
    }

    pub async fn get_topology_data(&self) -> HashMap<String, SwarmTopologyData> {
        self.topology_data.read().await.clone()
    }

    pub async fn get_global_performance_metrics(&self) -> GlobalPerformanceMetrics {
        let agents = self.discovered_agents.read().await;
        let agent_list: Vec<&MultiMcpAgentStatus> = agents.values().collect();

        if agent_list.is_empty() {
            return GlobalPerformanceMetrics {
                total_throughput: 0.0,
                average_latency: 0.0,
                system_efficiency: 0.0,
                resource_utilization: 0.0,
                error_rate: 0.0,
                coordination_overhead: 0.0,
            };
        }

        let total_throughput: f32 = agent_list.iter().map(|a| a.performance.throughput).sum();
        let average_latency: f32 = agent_list
            .iter()
            .map(|a| a.performance.response_time_ms)
            .sum::<f32>()
            / agent_list.len() as f32;
        let resource_utilization: f32 = agent_list
            .iter()
            .map(|a| (a.performance.cpu_usage + a.performance.memory_usage) / 2.0)
            .sum::<f32>()
            / agent_list.len() as f32;

        let total_tasks: u32 = agent_list
            .iter()
            .map(|a| a.performance.tasks_completed + a.performance.tasks_failed)
            .sum();
        let failed_tasks: u32 = agent_list.iter().map(|a| a.performance.tasks_failed).sum();
        let error_rate = if total_tasks > 0 {
            failed_tasks as f32 / total_tasks as f32
        } else {
            0.0
        };

        GlobalPerformanceMetrics {
            total_throughput,
            average_latency,
            system_efficiency: (total_throughput / agent_list.len() as f32).min(1.0),
            resource_utilization,
            error_rate,
            coordination_overhead: self.calculate_coordination_overhead(&agent_list),
        }
    }

    fn calculate_coordination_overhead(&self, agents: &[&MultiMcpAgentStatus]) -> f32 {
        if agents.is_empty() {
            return 0.0;
        }

        let agent_count = agents.len() as f32;
        let avg_queue_size = agents
            .iter()
            .map(|a| a.metadata.task_queue_size as f32)
            .sum::<f32>()
            / agent_count;

        let mut server_types = std::collections::HashSet::new();
        for agent in agents {
            server_types.insert(std::mem::discriminant(&agent.server_source));
        }
        let server_diversity = server_types.len() as f32;

        let base_overhead = (agent_count.ln() / 10.0).min(0.3);

        let queue_overhead = (avg_queue_size / 10.0).min(0.2);

        let cross_server_overhead = if server_diversity > 1.0 {
            (server_diversity - 1.0) * 0.05
        } else {
            0.0
        };

        (base_overhead + queue_overhead + cross_server_overhead).min(0.8)
    }

    pub async fn is_any_server_online(&self) -> bool {
        self.server_statuses
            .read()
            .await
            .values()
            .any(|server| server.is_connected)
    }

    pub async fn get_total_agent_count(&self) -> u32 {
        self.discovered_agents.read().await.len() as u32
    }
}

impl Default for MultiMcpAgentDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod discovery_interval_tests {
    use super::*;

    fn config(discovery_interval_ms: u64, enabled: bool) -> McpServerConfig {
        McpServerConfig {
            server_id: "test".to_string(),
            server_type: McpServerType::Custom("test".to_string()),
            host: "localhost".to_string(),
            port: 0,
            enabled,
            discovery_interval_ms,
            timeout_ms: 1000,
            retry_attempts: 1,
        }
    }

    /// ADR-2083: the minimum discovery_interval_ms across enabled servers wins,
    /// so every server is polled at least as often as it configured.
    #[test]
    fn selects_minimum_across_enabled_servers() {
        let configs = [config(3000, true), config(5000, true), config(4000, true)];
        assert_eq!(select_discovery_interval_ms(configs.iter()), 3000);
    }

    /// ADR-2083: a configured value below the floor is clamped up, so a
    /// misconfigured discovery_interval_ms=0 cannot spin the loop.
    #[test]
    fn floor_is_applied_to_a_too_small_configured_value() {
        let configs = [config(0, true), config(50, true)];
        assert_eq!(
            select_discovery_interval_ms(configs.iter()),
            MIN_DISCOVERY_INTERVAL_MS
        );
    }

    /// ADR-2083: a value already above the floor passes through unchanged.
    #[test]
    fn value_above_floor_is_unchanged() {
        let configs = [config(1200, true)];
        assert_eq!(select_discovery_interval_ms(configs.iter()), 1200);
    }

    /// ADR-2083: disabled servers must never lower (or otherwise influence)
    /// the selected interval, even if their configured value is tiny.
    #[test]
    fn disabled_servers_are_ignored() {
        let configs = [config(100, false), config(6000, true)];
        assert_eq!(select_discovery_interval_ms(configs.iter()), 6000);
    }

    /// ADR-2083: an all-disabled or empty server set falls back to the
    /// named default rather than a bare literal or a busy-poll floor.
    #[test]
    fn falls_back_to_default_when_no_server_is_enabled() {
        let configs = [config(100, false), config(200, false)];
        assert_eq!(
            select_discovery_interval_ms(configs.iter()),
            DEFAULT_DISCOVERY_INTERVAL_MS
        );

        let empty: Vec<McpServerConfig> = Vec::new();
        assert_eq!(
            select_discovery_interval_ms(empty.iter()),
            DEFAULT_DISCOVERY_INTERVAL_MS
        );
    }
}
