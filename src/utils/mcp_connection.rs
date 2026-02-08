use log::{debug, error, info, warn};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

pub struct PersistentMCPConnection {
    stream: Arc<Mutex<TcpStream>>,
    session_id: String,
    initialized: bool,
}

impl PersistentMCPConnection {
    
    pub async fn new(
        host: &str,
        port: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", host, port);
        info!("Connecting to MCP server at {}", addr);

        let mut stream = TcpStream::connect(&addr).await?;
        info!("TCP connection established to MCP server");

        let session_id = Uuid::new_v4().to_string();

        
        let init_request = json!({
            "jsonrpc": "2.0",
            "id": session_id.clone(),
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "VisionFlow-BotsClient",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {
                        "listChanged": true
                    }
                }
            }
        });

        let msg = format!("{}\n", init_request.to_string());
        debug!("Sending MCP init: {}", msg.trim());
        stream.write_all(msg.as_bytes()).await?;
        stream.flush().await?;

        
        let mut buffer = Vec::new();
        let mut byte = [0u8; 1];

        
        loop {
            buffer.clear();

            
            loop {
                match tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut byte))
                    .await
                {
                    Ok(Ok(_)) => {
                        if byte[0] == b'\n' {
                            break;
                        }
                        buffer.push(byte[0]);
                    }
                    Ok(Err(e)) => {
                        error!("Error reading from stream: {}", e);
                        return Err(Box::new(e));
                    }
                    Err(_) => {
                        error!("Timeout reading MCP initialization response");
                        return Err("MCP initialization timeout".into());
                    }
                }
            }

            let response_line = String::from_utf8_lossy(&buffer);
            debug!("MCP response: {}", response_line.trim());

            
            if response_line.contains("server.initialized") {
                continue;
            }

            
            if let Ok(response) = serde_json::from_str::<Value>(&response_line) {
                if response.get("id").and_then(|id| id.as_str()) == Some(&session_id) {
                    if response.get("result").is_some() {
                        info!("MCP session initialized: {}", session_id);

                        return Ok(PersistentMCPConnection {
                            stream: Arc::new(Mutex::new(stream)),
                            session_id,
                            initialized: true,
                        });
                    } else if let Some(error) = response.get("error") {
                        error!("MCP init error: {:?}", error);
                        return Err(format!("MCP initialization failed: {:?}", error).into());
                    }
                }
            }
        }
    }

    
    pub async fn execute_command(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        if !self.initialized {
            return Err("Connection not initialized".into());
        }

        let request_id = Uuid::new_v4().to_string();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });

        let msg = format!("{}\n", request.to_string());
        debug!("Sending MCP command: {}", msg.trim());

        
        let mut stream = self.stream.lock().await;
        stream.write_all(msg.as_bytes()).await?;
        stream.flush().await?;

        
        let mut buffer = Vec::new();
        let mut byte = [0u8; 1];

        
        loop {
            buffer.clear();

            
            loop {
                match tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut byte))
                    .await
                {
                    Ok(Ok(_)) => {
                        if byte[0] == b'\n' {
                            break;
                        }
                        buffer.push(byte[0]);
                    }
                    Ok(Err(e)) => {
                        error!("Error reading from stream: {}", e);
                        return Err(Box::new(e));
                    }
                    Err(_) => {
                        error!("Timeout reading MCP response");
                        return Err("MCP response timeout".into());
                    }
                }
            }

            let response_line = String::from_utf8_lossy(&buffer);
            let trimmed = response_line.trim();

            if trimmed.is_empty() {
                continue;
            }

            debug!("MCP response: {}", trimmed);

            
            if trimmed.contains("server.initialized") {
                continue;
            }

            
            if let Ok(response) = serde_json::from_str::<Value>(trimmed) {
                
                if response.get("id").and_then(|id| id.as_str()) == Some(&request_id) {
                    if let Some(result) = response.get("result") {
                        info!("MCP command '{}' executed successfully", method);
                        return Ok(result.clone());
                    } else if let Some(error) = response.get("error") {
                        error!("MCP command error: {:?}", error);
                        return Err(format!("MCP error: {:?}", error).into());
                    }
                } else if response.get("method").is_some() {
                    
                    continue;
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct MCPConnectionPool {
    connections: Arc<RwLock<HashMap<String, Arc<PersistentMCPConnection>>>>,
    host: String,
    port: String,
    max_retries: u32,
    retry_delay: Duration,
}

impl MCPConnectionPool {
    pub fn new(host: String, port: String) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            host,
            port,
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
        }
    }

    
    pub async fn get_connection(
        &self,
        purpose: &str,
    ) -> Result<Arc<PersistentMCPConnection>, Box<dyn std::error::Error + Send + Sync>> {
        
        {
            let connections = self.connections.read().await;
            if let Some(conn) = connections.get(purpose) {
                debug!("Reusing existing MCP connection for {}", purpose);
                return Ok(Arc::clone(conn));
            }
        }

        
        info!("Creating new MCP connection for {}", purpose);

        for attempt in 1..=self.max_retries {
            info!("Connection attempt {}/{}", attempt, self.max_retries);

            match PersistentMCPConnection::new(&self.host, &self.port).await {
                Ok(conn) => {
                    let conn = Arc::new(conn);

                    
                    let mut connections = self.connections.write().await;
                    connections.insert(purpose.to_string(), Arc::clone(&conn));

                    info!("MCP connection established for {}", purpose);
                    return Ok(conn);
                }
                Err(e) => {
                    warn!("Failed to create connection (attempt {}): {}", attempt, e);
                    if attempt < self.max_retries {
                        tokio::time::sleep(self.retry_delay).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err("Failed to establish MCP connection after all retries".into())
    }

    
    pub async fn execute_command(
        &self,
        purpose: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let conn = self.get_connection(purpose).await?;
        conn.execute_command(method, params).await
    }

    
    pub async fn remove_connection(&self, purpose: &str) {
        let mut connections = self.connections.write().await;
        if connections.remove(purpose).is_some() {
            info!("Removed MCP connection for {}", purpose);
        }
    }
}

pub async fn call_swarm_init(
    host: &str,
    port: &str,
    topology: &str,
    max_agents: u32,
    strategy: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = MCPConnectionPool::new(host.to_string(), port.to_string());

    let params = json!({
        "name": "swarm_init",
        "arguments": {
            "topology": topology,
            "maxAgents": max_agents,
            "strategy": strategy
        }
    });

    pool.execute_command("swarm_init", "tools/call", params)
        .await
}

pub async fn call_agent_list(
    host: &str,
    port: &str,
    filter: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = MCPConnectionPool::new(host.to_string(), port.to_string());

    let params = json!({
        "name": "agent_list",
        "arguments": {
            "filter": filter
        }
    });

    pool.execute_command("agent_list", "tools/call", params)
        .await
}

pub async fn call_swarm_destroy(
    host: &str,
    port: &str,
    swarm_id: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = MCPConnectionPool::new(host.to_string(), port.to_string());

    info!("Calling swarm_destroy for swarm_id: {}", swarm_id);

    let params = json!({
        "name": "swarm_destroy",
        "arguments": {
            "swarmId": swarm_id
        }
    });

    pool.execute_command("swarm_destroy", "tools/call", params)
        .await
}

pub async fn call_agent_spawn(
    host: &str,
    port: &str,
    agent_type: &str,
    swarm_id: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = MCPConnectionPool::new(host.to_string(), port.to_string());

    info!(
        "Spawning agent of type: {} in swarm: {}",
        agent_type, swarm_id
    );

    let params = json!({
        "name": "agent_spawn",
        "arguments": {
            "type": agent_type,
            "swarmId": swarm_id
        }
    });

    pool.execute_command("agent_spawn", "tools/call", params)
        .await
}

pub async fn call_task_orchestrate(
    host: &str,
    port: &str,
    task: &str,
    priority: Option<&str>,
    strategy: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = MCPConnectionPool::new(host.to_string(), port.to_string());

    info!("Orchestrating task: {}", task);

    let mut args = json!({
        "task": task
    });

    if let Some(p) = priority {
        args["priority"] = json!(p);
    }

    if let Some(s) = strategy {
        args["strategy"] = json!(s);
    }

    let params = json!({
        "name": "task_orchestrate",
        "arguments": args
    });

    pool.execute_command("task_orchestrate", "tools/call", params)
        .await
}

#[derive(Debug, Clone)]
pub enum TaskMethod {
    Docker, 
    MCP,    
    Hybrid, 
}

// DEPRECATED: Legacy Docker orchestration functions removed
// Use TaskOrchestratorActor with Management API instead



pub async fn call_task_status(
    host: &str,
    port: &str,
    task_id: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = MCPConnectionPool::new(host.to_string(), port.to_string());

    info!("Getting task status for: {:?}", task_id);

    let mut args = json!({});

    if let Some(id) = task_id {
        args["taskId"] = json!(id);
    }

    let params = json!({
        "name": "task_status",
        "arguments": args
    });

    pool.execute_command("task_status", "tools/call", params)
        .await
}
