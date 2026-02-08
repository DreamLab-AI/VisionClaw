use actix::{Actor, AsyncContext};
use actix_web_actors:::ws;
use chrono:::Utc;
use log:::warn;
use serde_json:::json;
use std::time::{Duration, Instant};
use crate::utils:::time;
use crate::utils::json::to_json;

pub trait WebSocketHeartbeat: Actor<Context = ws::WebsocketContext<Self>>
where
    Self: Sized,
{
    
    fn get_client_id(&self) -> &str;

    
    fn get_last_heartbeat(&self) -> Instant;

    
    fn update_last_heartbeat(&mut self);

    
    fn start_heartbeat(
        &self,
        ctx: &mut ws::WebsocketContext<Self>,
        ping_interval_secs: u64,
        timeout_secs: u64,
    ) where
        Self: actix::Actor<Context = ws::WebsocketContext<Self>> + 'static,
    {
        let ping_duration = Duration::from_secs(ping_interval_secs);
        let timeout_duration = Duration::from_secs(timeout_secs);

        ctx.run_interval(ping_duration, move |act, ctx| {
            if Instant::now().duration_since(act.get_last_heartbeat()) > timeout_duration {
                warn!(
                    "WebSocket client {} heartbeat timeout, disconnecting",
                    act.get_client_id()
                );
                
                ctx.close(Some(ws::CloseReason {
                    code: ws::CloseCode::Abnormal,
                    description: Some("Heartbeat timeout".to_string()),
                }));
                return;
            }

            ctx.ping(b"heartbeat");
        });
    }

    
    fn send_ping(&self, ctx: &mut ws::WebsocketContext<Self>)
    where
        Self: actix::Actor<Context = ws::WebsocketContext<Self>>,
    {
        let ping_message = json!({
            "type": "ping",
            "timestamp": time::now(),
            "client_id": self.get_client_id()
        });

        if let Ok(msg) = to_json(&ping_message) {
            ctx.text(msg);
        }
    }

    
    fn send_pong(&self, ctx: &mut ws::WebsocketContext<Self>)
    where
        Self: actix::Actor<Context = ws::WebsocketContext<Self>>,
    {
        let pong_message = json!({
            "type": "pong",
            "timestamp": time::now(),
            "client_id": self.get_client_id()
        });

        if let Ok(msg) = to_json(&pong_message) {
            ctx.text(msg);
        }
    }

    
    fn handle_heartbeat_message(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut ws::WebsocketContext<Self>,
    ) -> bool
    where
        Self: actix::Actor<Context = ws::WebsocketContext<Self>>,
    {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.update_last_heartbeat();
                ctx.pong(&msg);
                true 
            }
            Ok(ws::Message::Pong(_)) => {
                self.update_last_heartbeat();
                true 
            }
            _ => false, 
        }
    }
}

pub struct HeartbeatConfig {
    pub ping_interval_secs: u64,
    pub timeout_secs: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            ping_interval_secs: 5, 
            timeout_secs: 30,      
        }
    }
}

impl HeartbeatConfig {
    pub fn new(ping_interval_secs: u64, timeout_secs: u64) -> Self {
        Self {
            ping_interval_secs,
            timeout_secs,
        }
    }

    pub fn fast() -> Self {
        Self::new(2, 10) 
    }

    pub fn slow() -> Self {
        Self::new(15, 60) 
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(tag = "type")]
pub enum CommonWebSocketMessage {
    #[serde(rename = "ping")]
    Ping {
        timestamp: chrono::DateTime<Utc>,
        client_id: String,
    },

    #[serde(rename = "pong")]
    Pong {
        timestamp: chrono::DateTime<Utc>,
        client_id: String,
    },

    #[serde(rename = "connection_established")]
    ConnectionEstablished {
        client_id: String,
        timestamp: chrono::DateTime<Utc>,
    },

    #[serde(rename = "error")]
    Error {
        message: String,
        timestamp: chrono::DateTime<Utc>,
    },
}

