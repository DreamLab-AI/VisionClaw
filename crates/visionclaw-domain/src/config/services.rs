use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use validator::Validate;

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AuthSettings {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[serde(alias = "provider")]
    pub provider: String,
    #[serde(alias = "required")]
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RagFlowSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "agent_id")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_base_url")]
    pub api_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "max_retries")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "chat_id")]
    pub chat_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PerplexitySettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "model")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "temperature")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "top_p")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "presence_penalty")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "frequency_penalty")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "rate_limit")]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct OpenAISettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "base_url")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "rate_limit")]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PocketTtsSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_voice")]
    pub default_voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_format")]
    pub default_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_speed")]
    pub default_speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "stream")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "return_timestamps")]
    pub return_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "sample_rate")]
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct WhisperSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_model")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_language")]
    pub default_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "temperature")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "return_timestamps")]
    pub return_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "vad_filter")]
    pub vad_filter: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "word_timestamps")]
    pub word_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "initial_prompt")]
    pub initial_prompt: Option<String>,
}

// Voice routing configuration for multi-user real-time audio
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRoutingSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "livekit")]
    pub livekit: Option<LiveKitSettings>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "turbo_whisper")]
    pub turbo_whisper: Option<TurboWhisperSettings>,
    /// Per-agent voice presets mapping agent_type -> PocketTts voice ID
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        alias = "agent_voices"
    )]
    pub agent_voices: HashMap<String, AgentVoicePreset>,
    /// Audio format for the entire pipeline (default: opus)
    #[serde(default = "default_audio_format", alias = "audio_format")]
    pub audio_format: String,
    /// Sample rate in Hz (default: 48000)
    #[serde(default = "default_sample_rate_48k", alias = "sample_rate")]
    pub sample_rate: u32,
    /// Push-to-talk mode: "push" (hold key) or "toggle" (press to start/stop)
    #[serde(default = "default_ptt_mode", alias = "ptt_mode")]
    pub ptt_mode: String,
    /// Whether agent responses are audible to all users (spatial) or owner-only
    #[serde(default, alias = "agent_voice_public")]
    pub agent_voice_public: bool,
}

fn default_audio_format() -> String {
    "opus".to_string()
}
fn default_sample_rate_48k() -> u32 {
    48000
}
fn default_ptt_mode() -> String {
    "push".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct LiveKitSettings {
    /// LiveKit server URL (default: ws://livekit:7880)
    #[serde(skip_serializing_if = "Option::is_none", alias = "server_url")]
    pub server_url: Option<String>,
    /// API key for token generation
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    /// API secret for token signing
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_secret")]
    pub api_secret: Option<String>,
    /// Room name template (default: "visionclaw-{world_id}")
    #[serde(skip_serializing_if = "Option::is_none", alias = "room_prefix")]
    pub room_prefix: Option<String>,
    /// Enable spatial audio based on Vircadia entity positions
    #[serde(default = "default_true", alias = "spatial_audio")]
    pub spatial_audio: bool,
    /// Max distance (in Vircadia units) before audio falls to zero
    #[serde(
        default = "default_spatial_max_distance",
        alias = "spatial_max_distance"
    )]
    pub spatial_max_distance: f32,
}

fn default_spatial_max_distance() -> f32 {
    50.0
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TurboWhisperSettings {
    /// Turbo Whisper streaming endpoint (default: ws://turbo-whisper:8000/v1/audio/transcriptions)
    #[serde(skip_serializing_if = "Option::is_none", alias = "ws_url")]
    pub ws_url: Option<String>,
    /// REST fallback URL (default: http://turbo-whisper:8000/v1/audio/transcriptions)
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    /// Model to use (default: Systran/faster-whisper-large-v3)
    #[serde(skip_serializing_if = "Option::is_none", alias = "model")]
    pub model: Option<String>,
    /// Language hint (default: en)
    #[serde(skip_serializing_if = "Option::is_none", alias = "language")]
    pub language: Option<String>,
    /// Enable VAD (voice activity detection) to skip silence
    #[serde(default = "default_true", alias = "vad_filter")]
    pub vad_filter: bool,
    /// Beam size (1 = greedy/fastest, 5 = more accurate)
    #[serde(default = "default_beam_size", alias = "beam_size")]
    pub beam_size: u32,
}

fn default_beam_size() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AgentVoicePreset {
    /// PocketTts voice ID (e.g., "alba", "am_adam", "bf_emma")
    pub voice_id: String,
    /// Speech speed multiplier (default: 1.0)
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Whether this agent's voice is heard spatially by all users
    #[serde(default)]
    pub spatial: bool,
}

fn default_speed() -> f32 {
    1.0
}

// ---------- Ontology Agent Settings ----------

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct OntologyAgentSettings {
    /// Minimum quality score for auto-merging agent proposals (0.0-1.0)
    #[serde(default = "default_auto_merge_threshold")]
    pub auto_merge_threshold: f32,
    /// Minimum confidence for agent proposals to create a PR (0.0-1.0)
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
    /// Maximum discovery results per query
    #[serde(default = "default_max_discovery_results")]
    pub max_discovery_results: usize,
    /// Whether to require Whelk consistency check before creating PRs
    #[serde(default = "default_true")]
    pub require_consistency_check: bool,
    /// GitHub repository owner for ontology PRs (overrides env GITHUB_REPO_OWNER)
    #[serde(skip_serializing_if = "Option::is_none", alias = "github_owner")]
    pub github_owner: Option<String>,
    /// GitHub repository name for ontology PRs (overrides env GITHUB_REPO_NAME)
    #[serde(skip_serializing_if = "Option::is_none", alias = "github_repo")]
    pub github_repo: Option<String>,
    /// Base branch for ontology PRs (default: main)
    #[serde(skip_serializing_if = "Option::is_none", alias = "github_base_branch")]
    pub github_base_branch: Option<String>,
    /// Path prefix for per-user ontology notes in the repo
    #[serde(default = "default_notes_path_prefix", alias = "notes_path_prefix")]
    pub notes_path_prefix: String,
    /// Labels to add to ontology PRs
    #[serde(default = "default_pr_labels", alias = "pr_labels")]
    pub pr_labels: Vec<String>,
    /// SHACL ingest-gate mode (PRD-022 WS-1): `"enforcing"` rejects an ingest
    /// on any `sh:Violation` shape finding; `"advisory"` records findings but
    /// lets the write proceed. Default `"enforcing"`; set `"advisory"` to roll
    /// back. Consumed at startup by `shacl_gate::set_global_gate_mode`.
    #[serde(default = "default_shacl_mode", alias = "shacl_mode")]
    pub shacl_mode: String,
}

fn default_shacl_mode() -> String {
    "enforcing".to_string()
}
fn default_auto_merge_threshold() -> f32 {
    0.9
}
fn default_min_confidence() -> f32 {
    0.7
}
fn default_max_discovery_results() -> usize {
    20
}
fn default_notes_path_prefix() -> String {
    "pages/".to_string()
}
fn default_pr_labels() -> Vec<String> {
    vec!["ontology".to_string(), "agent-proposed".to_string()]
}
