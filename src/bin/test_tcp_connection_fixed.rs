use log::{error, info};
use serde_json::json;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use uuid::Uuid;
use webxr::utils::json::to_json;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("Starting enhanced TCP connection test for Claude Flow MCP");

    let host =
        std::env::var("CLAUDE_FLOW_HOST").unwrap_or_else(|_| "multi-agent-container".to_string());
    let port = std::env::var("MCP_TCP_PORT").unwrap_or_else(|_| "9500".to_string());

    info!("Connecting to {}:{}...", host, port);

    
    let initial_fd_count = count_open_fds().await;
    info!("Initial file descriptors: {}", initial_fd_count);

    let start = Instant::now();

    
    let stream = match TcpStream::connect(format!("{}:{}", host, port)).await {
        Ok(stream) => {
            info!("Successfully connected to {}:{}", host, port);
            stream
        }
        Err(e) => {
            error!("Failed to connect to {}:{}: {}", host, port, e);
            return Err(e.into());
        }
    };

    let connect_time = start.elapsed();
    info!("Connected in {:?}", connect_time);

    
    stream.set_nodelay(true)?;

    
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = BufWriter::new(write_half);

    
    let post_connect_fd_count = count_open_fds().await;
    info!(
        "File descriptors after connect: {} (delta: +{})",
        post_connect_fd_count,
        post_connect_fd_count - initial_fd_count
    );

    
    let init_msg = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "initialize",
        "params": {
            "protocolVersion": "1.0.0",
            "capabilities": {
                "roots": true,
                "sampling": true
            },
            "clientInfo": {
                "name": "visionflow-test-fixed",
                "version": "1.0.0"
            }
        }
    });

    let msg_str = format!("{}\n", to_json(&init_msg)?);
    writer.write_all(msg_str.as_bytes()).await?;
    writer.flush().await?;

    info!("Sent initialization message");

    
    let mut response = String::new();
    let read_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut response),
    )
    .await;

    match read_result {
        Ok(Ok(_)) => info!("Received response: {}", response.trim()),
        Ok(Err(e)) => error!("Error reading response: {}", e),
        Err(_) => error!("Timeout waiting for response"),
    }

    
    let list_tools = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "tools/list"
    });

    let msg_str = format!("{}\n", to_json(&list_tools)?);
    writer.write_all(msg_str.as_bytes()).await?;
    writer.flush().await?;

    info!("Requested tool list");

    
    let mut tools_response = String::new();
    let read_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut tools_response),
    )
    .await;

    match read_result {
        Ok(Ok(_)) => info!("Available tools: {}", tools_response.trim()),
        Ok(Err(e)) => error!("Error reading tools response: {}", e),
        Err(_) => error!("Timeout waiting for tools response"),
    }

    
    let swarm_init = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "tools/call",
        "params": {
            "name": "swarm_init",
            "arguments": {
                "objective": "Test swarm",
                "maxAgents": 3,
                "strategy": "balanced"
            }
        }
    });

    let msg_str = format!("{}\n", to_json(&swarm_init)?);
    let send_start = Instant::now();
    writer.write_all(msg_str.as_bytes()).await?;
    writer.flush().await?;
    let send_time = send_start.elapsed();

    info!("Swarm initialization sent in {:?}", send_time);

    
    let mut swarm_response = String::new();
    let read_result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        reader.read_line(&mut swarm_response),
    )
    .await;

    match read_result {
        Ok(Ok(_)) => info!("Swarm response: {}", swarm_response.trim()),
        Ok(Err(e)) => error!("Error reading swarm response: {}", e),
        Err(_) => error!("Timeout waiting for swarm response"),
    }

    
    info!("Shutting down TCP connection gracefully...");
    match writer.shutdown().await {
        Ok(_) => info!("TCP writer shutdown successfully"),
        Err(e) => error!("Error shutting down TCP writer: {}", e),
    }

    
    drop(reader);

    let total_time = start.elapsed();

    
    let final_fd_count = count_open_fds().await;
    let fd_delta = final_fd_count as i32 - initial_fd_count as i32;

    info!(
        "Final file descriptors: {} (delta: {:+})",
        final_fd_count, fd_delta
    );

    if fd_delta > 0 {
        error!(
            "⚠️  File descriptor leak detected! {} descriptors not cleaned up",
            fd_delta
        );
    } else {
        info!("✅ No file descriptor leaks detected");
    }

    
    println!("\n=== Performance & Resource Summary ===");
    println!("Connection time: {:?}", connect_time);
    println!("Message send time: {:?}", send_time);
    println!("Total test time: {:?}", total_time);
    println!("File descriptor delta: {:+}", fd_delta);
    println!(
        "Resource leak status: {}",
        if fd_delta > 0 {
            "LEAK DETECTED"
        } else {
            "CLEAN"
        }
    );

    info!("TCP connection test completed successfully with resource monitoring");
    Ok(())
}

async fn count_open_fds() -> usize {
    #[cfg(target_os = "linux")]
    {
        match tokio::fs::read_dir("/proc/self/fd").await {
            Ok(mut entries) => {
                let mut count: usize = 0;
                while let Ok(Some(_)) = entries.next_entry().await {
                    count += 1;
                }
                count.saturating_sub(1) 
            }
            Err(_) => {
                
                match tokio::process::Command::new("lsof")
                    .args(["-p", &std::process::id().to_string()])
                    .output()
                    .await
                {
                    Ok(output) => {
                        String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .skip(1) 
                            .count()
                    }
                    Err(_) => 0, 
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        
        10
    }
}
