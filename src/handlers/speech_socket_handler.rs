use crate::actors::messages::GetSettings;
use crate::app_state::AppState;
use crate::types::speech::SpeechOptions;
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
// DEPRECATED: HybridHealthManager removed - use TaskOrchestratorActor instead
use futures::FutureExt;
use tokio::sync::broadcast;

// Constants for heartbeat
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

// Define message types
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextToSpeechRequest {
    text: String,
    voice: Option<String>,
    speed: Option<f32>,
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetProviderRequest {
    provider: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct STTActionRequest {
    action: String, 
    language: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceCommandRequest {
    text: String,
    session_id: Option<String>,
    respond_via_voice: Option<bool>,
}

pub struct SpeechSocket {
    id: String,
    app_state: Arc<AppState>,
    _hybrid_manager: Option<()>, 
    heartbeat: Instant,
    audio_rx: Option<broadcast::Receiver<Vec<u8>>>,
    transcription_rx: Option<broadcast::Receiver<String>>,
}

impl SpeechSocket {
    pub fn new(id: String, app_state: Arc<AppState>, _hybrid_manager: Option<()>) -> Self {
        let (audio_rx, transcription_rx) = if let Some(speech_service) = &app_state.speech_service {
            (
                Some(speech_service.subscribe_to_audio()),
                Some(speech_service.subscribe_to_transcriptions()),
            )
        } else {
            (None, None)
        };

        Self {
            id,
            app_state,
            _hybrid_manager: None,
            heartbeat: Instant::now(),
            audio_rx,
            transcription_rx,
        }
    }

    
    fn start_heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > CLIENT_TIMEOUT {
                info!("SpeechSocket client heartbeat failed, disconnecting!");
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }

    
    async fn process_tts_request(
        app_state: Arc<AppState>,
        req: TextToSpeechRequest,
    ) -> Result<(), String> {
        if let Some(speech_service) = &app_state.speech_service {
            
            let settings = app_state
                .settings_addr
                .send(GetSettings)
                .await
                .map_err(|e| format!("Settings actor mailbox error: {}", e))?
                .map_err(|e| format!("Failed to get settings: {}", e))?;
            let kokoro_config = settings.kokoro.as_ref(); 

            
            let default_voice = kokoro_config
                .and_then(|k| k.default_voice.clone())
                .unwrap_or_else(|| {
                    
                    "af_sarah".to_string() 
                });
            let default_speed = kokoro_config.and_then(|k| k.default_speed).unwrap_or(1.0);
            let default_stream = kokoro_config.and_then(|k| k.stream).unwrap_or(true); 

            
            let options = SpeechOptions {
                voice: req.voice.unwrap_or(default_voice),
                speed: req.speed.unwrap_or(default_speed),
                stream: req.stream.unwrap_or(default_stream),
            };

            
            match speech_service.text_to_speech(req.text, options).await {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Failed to process TTS request: {}", e)),
            }
        } else {
            Err("Speech service is not available".to_string())
        }
    }

    
    fn is_swarm_command(&self, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        text_lower.contains("swarm")
            || text_lower.contains("spawn agents")
            || text_lower.contains("create hive")
            || text_lower.contains("start swarm")
            || text_lower.contains("stop swarm")
            || text_lower.contains("agent status")
            || text_lower.contains("docker hive")
    }

    
    fn handle_swarm_voice_command(&self, _text: &str, ctx: &mut ws::WebsocketContext<Self>) {
        
        let error_msg = json!({
            "type": "error",
            "message": "Swarm voice commands deprecated - use API endpoints instead"
        })
        .to_string();
        ctx.text(error_msg);

        
    }
}

impl Actor for SpeechSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("[SpeechSocket] Client connected: {}", self.id);

        
        self.start_heartbeat(ctx);

        
        let welcome = json!({
            "type": "connected",
            "message": "Connected to speech service"
        });

        ctx.text(welcome.to_string());

        
        if let Some(mut rx) = self.audio_rx.take() {
            let addr = ctx.address();

            ctx.spawn(Box::pin(
                async move {
                    while let Ok(audio_data) = rx.recv().await {
                        
                        if addr.try_send(AudioChunkMessage(audio_data)).is_err() {
                            break;
                        }
                    }
                }
                .into_actor(self),
            ));
        }

        
        if let Some(mut rx) = self.transcription_rx.take() {
            let addr = ctx.address();

            ctx.spawn(Box::pin(
                async move {
                    while let Ok(transcription_text) = rx.recv().await {
                        
                        if addr
                            .try_send(TranscriptionMessage(transcription_text))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                .into_actor(self),
            ));
        }
    }
}

// Message type for audio data
struct AudioChunkMessage(Vec<u8>);

impl Message for AudioChunkMessage {
    type Result = ();
}

impl Handler<AudioChunkMessage> for SpeechSocket {
    type Result = ();

    fn handle(&mut self, msg: AudioChunkMessage, ctx: &mut Self::Context) -> Self::Result {
        
        ctx.binary(msg.0);
    }
}

// Message type for transcription data
struct TranscriptionMessage(String);

impl Message for TranscriptionMessage {
    type Result = ();
}

impl Handler<TranscriptionMessage> for SpeechSocket {
    type Result = ();

    fn handle(&mut self, msg: TranscriptionMessage, ctx: &mut Self::Context) -> Self::Result {
        
        let message = json!({
            "type": "transcription",
            "data": {
                "text": msg.0,
                "isFinal": true,
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            }
        });
        ctx.text(message.to_string());
    }
}

// Message type for error data
struct ErrorMessage(String);

impl Message for ErrorMessage {
    type Result = ();
}

impl Handler<ErrorMessage> for SpeechSocket {
    type Result = ();

    fn handle(&mut self, msg: ErrorMessage, ctx: &mut Self::Context) -> Self::Result {
        
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for SpeechSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.heartbeat = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                debug!("[SpeechSocket] Received text: {}", text);
                self.heartbeat = Instant::now();

                
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(msg) => {
                        
                        let msg_type = msg.get("type").and_then(|t| t.as_str());
                        match msg_type {
                            Some("tts") => {
                                
                                if let Ok(tts_req) =
                                    serde_json::from_value::<TextToSpeechRequest>(msg)
                                {
                                    
                                    let app_state = self.app_state.clone();
                                    let addr = ctx.address();
                                    let fut = async move {
                                        if let Err(e) =
                                            Self::process_tts_request(app_state, tts_req).await
                                        {
                                            let error_msg = json!({
                                                "type": "error",
                                                "message": e
                                            });
                                            let _ =
                                                addr.try_send(ErrorMessage(error_msg.to_string()));
                                        }
                                    };
                                    ctx.spawn(fut.into_actor(self));
                                } else {
                                    ctx.text(json!({"type": "error", "message": "Invalid TTS request format"}).to_string());
                                }
                            }
                            Some("stt") => {
                                
                                if let Ok(stt_req) = serde_json::from_value::<STTActionRequest>(msg)
                                {
                                    match stt_req.action.as_str() {
                                        "start" => {
                                            if let Some(speech_service) =
                                                &self.app_state.speech_service
                                            {
                                                use crate::types::speech::TranscriptionOptions;
                                                let options = TranscriptionOptions {
                                                    language: stt_req.language,
                                                    model: stt_req.model,
                                                    temperature: None,
                                                    stream: true,
                                                };

                                                let speech_service = speech_service.clone();
                                                let addr = ctx.address();
                                                let fut = async move {
                                                    match speech_service
                                                        .start_transcription(options)
                                                        .await
                                                    {
                                                        Ok(_) => {
                                                            let msg = json!({
                                                                "type": "stt_started",
                                                                "message": "Transcription started"
                                                            })
                                                            .to_string();
                                                            let _ =
                                                                addr.try_send(ErrorMessage(msg));
                                                        }
                                                        Err(e) => {
                                                            let msg = json!({
                                                                "type": "error",
                                                                "message": format!("Failed to start transcription: {}", e)
                                                            }).to_string();
                                                            let _ =
                                                                addr.try_send(ErrorMessage(msg));
                                                        }
                                                    }
                                                };
                                                ctx.spawn(fut.into_actor(self));
                                            }
                                        }
                                        "stop" => {
                                            if let Some(speech_service) =
                                                &self.app_state.speech_service
                                            {
                                                let speech_service = speech_service.clone();
                                                let addr = ctx.address();
                                                let fut = async move {
                                                    match speech_service.stop_transcription().await
                                                    {
                                                        Ok(_) => {
                                                            let msg = json!({
                                                                "type": "stt_stopped",
                                                                "message": "Transcription stopped"
                                                            })
                                                            .to_string();
                                                            let _ =
                                                                addr.try_send(ErrorMessage(msg));
                                                        }
                                                        Err(e) => {
                                                            let msg = json!({
                                                                "type": "error",
                                                                "message": format!("Failed to stop transcription: {}", e)
                                                            }).to_string();
                                                            let _ =
                                                                addr.try_send(ErrorMessage(msg));
                                                        }
                                                    }
                                                };
                                                ctx.spawn(fut.into_actor(self));
                                            }
                                        }
                                        _ => {
                                            ctx.text(json!({"type": "error", "message": "Invalid STT action"}).to_string());
                                        }
                                    }
                                } else {
                                    ctx.text(json!({"type": "error", "message": "Invalid STT request format"}).to_string());
                                }
                            }
                            Some("voice_command") => {
                                
                                if let Ok(voice_req) =
                                    serde_json::from_value::<VoiceCommandRequest>(msg)
                                {
                                    
                                    if self.is_swarm_command(&voice_req.text) {
                                        self.handle_swarm_voice_command(&voice_req.text, ctx);
                                    } else if let Some(speech_service) =
                                        &self.app_state.speech_service
                                    {
                                        let speech_service = speech_service.clone();
                                        let addr = ctx.address();
                                        let fut = async move {
                                            
                                            let session_id =
                                                voice_req.session_id.unwrap_or_else(|| {
                                                    uuid::Uuid::new_v4().to_string()
                                                });

                                            match speech_service
                                                .process_voice_command_with_tags(
                                                    voice_req.text.clone(),
                                                    session_id,
                                                )
                                                .await
                                            {
                                                Ok(response) => {
                                                    let msg = json!({
                                                        "type": "voice_response",
                                                        "data": {
                                                            "text": response,
                                                            "isFinal": true,
                                                            "timestamp": std::time::SystemTime::now()
                                                                .duration_since(std::time::UNIX_EPOCH)
                                                                .unwrap_or_default()
                                                                .as_millis()
                                                        }
                                                    }).to_string();
                                                    let _ = addr.try_send(ErrorMessage(msg));
                                                }
                                                Err(_) => {
                                                    
                                                    match speech_service
                                                        .process_voice_command(voice_req.text)
                                                        .await
                                                    {
                                                        Ok(response) => {
                                                            let msg = json!({
                                                                "type": "voice_response",
                                                                "data": {
                                                                    "text": response,
                                                                    "isFinal": true,
                                                                    "timestamp": std::time::SystemTime::now()
                                                                        .duration_since(std::time::UNIX_EPOCH)
                                                                        .unwrap_or_default()
                                                                        .as_millis()
                                                                }
                                                            }).to_string();
                                                            let _ =
                                                                addr.try_send(ErrorMessage(msg));
                                                        }
                                                        Err(e) => {
                                                            let msg = json!({
                                                                "type": "error",
                                                                "message": format!("Voice command failed: {}", e)
                                                            }).to_string();
                                                            let _ =
                                                                addr.try_send(ErrorMessage(msg));
                                                        }
                                                    }
                                                }
                                            }
                                        };
                                        ctx.spawn(fut.into_actor(self));
                                    } else {
                                        ctx.text(json!({"type": "error", "message": "Speech service not available"}).to_string());
                                    }
                                } else {
                                    ctx.text(json!({"type": "error", "message": "Invalid voice command format"}).to_string());
                                }
                            }
                            _ => {
                                ctx.text(
                                    json!({"type": "error", "message": "Unknown message type"})
                                        .to_string(),
                                );
                            }
                        }
                    }
                    Err(e) => {
                        ctx.text(
                            json!({"type": "error", "message": format!("Invalid JSON: {}", e)})
                                .to_string(),
                        );
                    }
                }
            }
            Ok(ws::Message::Binary(bin)) => {
                debug!(
                    "[SpeechSocket] Received binary audio data: {} bytes",
                    bin.len()
                );
                self.heartbeat = Instant::now();

                
                if let Some(speech_service) = &self.app_state.speech_service {
                    let audio_data = bin.to_vec();

                    
                    let speech_service = speech_service.clone();
                    let fut = async move {
                        if let Err(e) = speech_service.process_audio_chunk(audio_data).await {
                            error!("Failed to process audio chunk: {}", e);
                        }
                    }
                    .boxed()
                    .into_actor(self);

                    ctx.spawn(fut);
                }
            }
            Ok(ws::Message::Close(reason)) => {
                info!("[SpeechSocket] Client disconnected: {}", self.id);
                ctx.close(reason);
                ctx.stop();
            }
            _ => (),
        }
    }
}

// Handler for the WebSocket route
pub async fn speech_socket_handler(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<AppState>,
    _hybrid_manager: Option<()>, 
) -> Result<HttpResponse, actix_web::Error> {
    let socket_id = format!("speech_{}", uuid::Uuid::new_v4());
    let socket = SpeechSocket::new(socket_id, app_state.into_inner(), None);

    match ws::start(socket, &req, stream) {
        Ok(response) => {
            info!("[SpeechSocket] WebSocket connection established");
            Ok(response)
        }
        Err(e) => {
            error!("[SpeechSocket] Failed to start WebSocket: {}", e);
            Err(e)
        }
    }
}
