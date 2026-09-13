use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum SpeechError {
    WebSocketError(tungstenite::Error),
    ConnectionError(String),
    SendError(mpsc::error::SendError<SpeechCommand>),
    SerializationError(serde_json::Error),
    ProcessError(std::io::Error),
    Base64Error(base64::DecodeError),
    BroadcastError(String),
    TTSError(String),
}

impl fmt::Display for SpeechError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpeechError::WebSocketError(e) => write!(f, "WebSocket error: {}", e),
            SpeechError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            SpeechError::SendError(e) => write!(f, "Send error: {}", e),
            SpeechError::SerializationError(e) => write!(f, "Serialization error: {}", e),
            SpeechError::ProcessError(e) => write!(f, "Process error: {}", e),
            SpeechError::Base64Error(e) => write!(f, "Base64 error: {}", e),
            SpeechError::BroadcastError(msg) => write!(f, "Broadcast error: {}", msg),
            SpeechError::TTSError(msg) => write!(f, "TTS error: {}", msg),
        }
    }
}

impl Error for SpeechError {}

impl From<tungstenite::Error> for SpeechError {
    fn from(err: tungstenite::Error) -> Self {
        SpeechError::WebSocketError(err)
    }
}

impl From<mpsc::error::SendError<SpeechCommand>> for SpeechError {
    fn from(err: mpsc::error::SendError<SpeechCommand>) -> Self {
        SpeechError::SendError(err)
    }
}

impl From<serde_json::Error> for SpeechError {
    fn from(err: serde_json::Error) -> Self {
        SpeechError::SerializationError(err)
    }
}

impl From<std::io::Error> for SpeechError {
    fn from(err: std::io::Error) -> Self {
        SpeechError::ProcessError(err)
    }
}

impl From<base64::DecodeError> for SpeechError {
    fn from(err: base64::DecodeError) -> Self {
        SpeechError::Base64Error(err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TTSProvider {
    PocketTts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum STTProvider {
    Whisper,
    /// Turbo Whisper (faster-whisper with streaming WebSocket)
    TurboWhisper,
    OpenAI,
}

#[derive(Debug)]
pub enum SpeechCommand {
    Initialize,
    SendMessage(String),
    TextToSpeech(String, SpeechOptions),
    /// User-scoped TTS: route audio only to the specified user
    TextToSpeechForUser(String, SpeechOptions, String),
    /// Agent spatial TTS: synthesize and inject into LiveKit at agent's position
    TextToSpeechSpatial(String, SpeechOptions, AgentSpatialInfo),
    Close,
    SetTTSProvider(TTSProvider),
    SetSTTProvider(STTProvider),
    StartTranscription(TranscriptionOptions),
    StopTranscription,
    ProcessAudioChunk(Vec<u8>),
    /// User-scoped audio processing: transcription routed to specific user's agents
    ProcessAudioChunkForUser(Vec<u8>, String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechOptions {
    pub voice: String,
    pub speed: f32,
    pub stream: bool,
    /// Output format: "opus" (default), "mp3", "wav"
    #[serde(default = "default_opus_format")]
    pub format: String,
}

fn default_opus_format() -> String {
    "opus".to_string()
}

impl Default for SpeechOptions {
    fn default() -> Self {
        Self {
            voice: "alba".to_string(),
            speed: 1.0,
            stream: true,
            format: "opus".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionOptions {
    pub language: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub stream: bool,
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self {
            language: None,
            model: Some("whisper-1".to_string()),
            temperature: None,
            stream: true,
        }
    }
}

/// Spatial position info for injecting agent voice into the LiveKit SFU
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpatialInfo {
    /// Agent identifier
    pub agent_id: String,
    /// 3D position in Vircadia world coordinates
    pub position: [f32; 3],
    /// Owner user ID (for private fallback)
    pub owner_user_id: String,
    /// Whether audio should be public (spatial) or private (owner only)
    pub public: bool,
}

/// Audio routing target — where synthesized audio should be delivered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioTarget {
    /// Send to all connected clients (legacy broadcast)
    Broadcast,
    /// Send only to a specific user's WebSocket session
    User(String),
    /// Inject into LiveKit room as spatial audio at a position
    Spatial {
        room: String,
        participant_id: String,
        position: [f32; 3],
    },
    /// Send to specific user AND inject spatially
    UserAndSpatial {
        user_id: String,
        room: String,
        participant_id: String,
        position: [f32; 3],
    },
}
