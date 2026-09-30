/// Image Generation Handler
/// Submits image jobs to local ComfyUI, saves results to the user's Solid pod.
///
/// Model selection (`IMAGE_GEN_MODEL`):
///   `minimax-h3` (default) — MiniMax H3 text-to-video, rendering the shortest
///       clip the model allows and saving frame 0 as the still. These are the
///       weights the estate ComfyUI serves today.
///   `flux2` — FLUX 2 Dev still-image graph; needs `flux2_dev_fp8mixed`,
///       `mistral_3_small_flux2_fp8` and `flux2-vae` installed in ComfyUI.
///
/// Flow (user session):
///   POST /api/image-gen/submit → build workflow → ComfyUI :8188/prompt
///   → poll /history → fetch PNG → PUT to /api/solid/pods/{user}/images/
///   → return { job_id, pod_image_url, width, height, seed }
///
/// Flow (agent / MCP):
///   POST /api/image-gen/agent-submit  (X-Agent-Key auth, no user session)
///   → same ComfyUI pipeline → PUT to embedded solid-pod-rs for target user_npub pod
///   → return { job_id, pod_image_url, comfyui_filename, seed }
///
///   GET  /api/image-gen/status/{job_id} → proxy to ComfyUI /history/{job_id}
use actix_web::{web, HttpRequest, HttpResponse};
use log::{error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(feature = "solid-pod-embed")]
use solid_pod_rs::Storage;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

use crate::services::nostr_service::NostrService;

// ─── env helpers ──────────────────────────────────────────────────────────────

fn comfyui_base() -> String {
    std::env::var("COMFYUI_URL").unwrap_or_else(|_| "http://comfyui:8188".to_string())
}

fn solid_base() -> String {
    // Internal base — goes through nginx→Rust solid proxy
    std::env::var("SOLID_INTERNAL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:4001/api/solid".to_string())
}

/// Constant-time byte comparison, so a timing side channel cannot recover the
/// agent key one byte at a time. Dependency-free fold — `subtle` and
/// `constant_time_eq` are only transitive deps here. Mirrors
/// `liveness_harness_handler::constant_time_eq` (ADR-2093).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Pure credential check, split out so the fail-closed semantics are unit
/// testable without constructing an `HttpRequest`.
///
/// ADR-2093: authorised **only** when a non-empty `VISIONCLAW_AGENT_KEY` is
/// configured and the request presents an exactly matching `X-Agent-Key`. An
/// unset or empty key fails closed — it is never substituted with a default, so
/// an unconfigured deployment cannot be driven with a publicly-known literal.
fn check_agent_key(expected: Option<&str>, provided: Option<&str>) -> bool {
    match expected.filter(|s| !s.is_empty()) {
        Some(key) => match provided {
            Some(got) => constant_time_eq(key.as_bytes(), got.as_bytes()),
            None => false,
        },
        None => false,
    }
}

/// Release posture: the key must be configured, and it is compared in constant
/// time. There is no bypass codepath here (ADR-2093, estate fail-closed posture).
#[cfg(not(any(debug_assertions, feature = "dev-auth")))]
fn agent_key_authorised(provided: Option<&str>) -> bool {
    check_agent_key(
        std::env::var("VISIONCLAW_AGENT_KEY").ok().as_deref(),
        provided,
    )
}

/// Dev / `dev-auth` builds keep the unauthenticated agent-submit flow, matching
/// the bypass `liveness_harness_handler` already uses for the same threat model.
/// This codepath does not exist in a release build.
#[cfg(any(debug_assertions, feature = "dev-auth"))]
fn agent_key_authorised(_provided: Option<&str>) -> bool {
    true
}

/// Which ComfyUI model graph `/submit` builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageModel {
    MiniMaxH3,
    Flux2,
}

impl ImageModel {
    /// Parse `IMAGE_GEN_MODEL`; unset or unrecognised values select MiniMax H3.
    fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("flux2") | Some("flux-2") => ImageModel::Flux2,
            _ => ImageModel::MiniMaxH3,
        }
    }

    fn from_env() -> Self {
        Self::parse(std::env::var("IMAGE_GEN_MODEL").ok().as_deref())
    }
}

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ImageGenRequest {
    /// The text prompt to generate
    pub prompt: String,
    /// Negative prompt (optional, unused by Flux2 and MiniMax H3 but stored for future use)
    #[serde(default)]
    pub negative_prompt: Option<String>,
    /// Image width (default 1024)
    #[serde(default = "default_width")]
    pub width: u32,
    /// Image height (default 1024)
    #[serde(default = "default_height")]
    pub height: u32,
    /// Number of steps (default 20)
    #[serde(default = "default_steps")]
    pub steps: u32,
    /// CFG / guidance (default 3.5 for Flux2; ignored by MiniMax H3)
    #[serde(default = "default_guidance")]
    pub guidance: f32,
    /// Random seed (-1 = random)
    #[serde(default = "default_seed")]
    pub seed: i64,
    /// Target subfolder inside pod (default "images")
    #[serde(default = "default_folder")]
    pub pod_folder: String,
}

fn default_width() -> u32 {
    1024
}
fn default_height() -> u32 {
    1024
}
fn default_steps() -> u32 {
    20
}
fn default_guidance() -> f32 {
    3.5
}
fn default_seed() -> i64 {
    -1
}
fn default_folder() -> String {
    "images".to_string()
}

#[derive(Debug, Serialize)]
pub struct ImageGenResponse {
    pub job_id: String,
    pub status: String,
    pub pod_image_url: Option<String>,
    pub comfyui_filename: Option<String>,
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub status: String,
    pub outputs: Option<Value>,
}

/// Request body for agent/MCP endpoint (no Nostr session required)
#[derive(Debug, Deserialize)]
pub struct AgentImageGenRequest {
    pub prompt: String,
    /// Nostr pubkey of the target pod owner (images stored under their pod)
    pub user_npub: Option<String>,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_steps")]
    pub steps: u32,
    #[serde(default = "default_guidance")]
    pub guidance: f32,
    #[serde(default = "default_seed")]
    pub seed: i64,
    #[serde(default = "default_folder")]
    pub pod_folder: String,
}

// ─── Flux2 workflow builder ───────────────────────────────────────────────────

fn build_flux2_workflow(req: &ImageGenRequest, seed: u64, filename_prefix: &str) -> Value {
    // Node IDs as strings (ComfyUI convention)
    // 1: UNETLoader  2: CLIPLoader  3: VAELoader
    // 4: CLIPTextEncode  5: FluxGuidance  6: BasicGuider
    // 7: RandomNoise  8: EmptyLatentImage (Flux2)  9: BasicScheduler
    // 10: SamplerCustomAdvanced  11: VAEDecode  12: SaveImage
    json!({
        "1": {
            "class_type": "UNETLoader",
            "inputs": {
                "unet_name": "flux2_dev_fp8mixed.safetensors",
                "weight_dtype": "fp8_e4m3fn"
            }
        },
        "2": {
            "class_type": "CLIPLoader",
            "inputs": {
                "clip_name": "mistral_3_small_flux2_fp8.safetensors",
                "type": "flux2"
            }
        },
        "3": {
            "class_type": "VAELoader",
            "inputs": {
                "vae_name": "flux2-vae.safetensors"
            }
        },
        "4": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": req.prompt,
                "clip": ["2", 0]
            }
        },
        "5": {
            "class_type": "FluxGuidance",
            "inputs": {
                "conditioning": ["4", 0],
                "guidance": req.guidance
            }
        },
        "6": {
            "class_type": "BasicGuider",
            "inputs": {
                "model": ["1", 0],
                "conditioning": ["5", 0]
            }
        },
        "7": {
            "class_type": "RandomNoise",
            "inputs": {
                "noise_seed": seed
            }
        },
        "8": {
            "class_type": "EmptySD3LatentImage",
            "inputs": {
                "width": req.width,
                "height": req.height,
                "batch_size": 1
            }
        },
        "9": {
            "class_type": "BasicScheduler",
            "inputs": {
                "model": ["1", 0],
                "scheduler": "beta",
                "steps": req.steps,
                "denoise": 1.0
            }
        },
        "13": {
            "class_type": "KSamplerSelect",
            "inputs": {
                "sampler_name": "euler"
            }
        },
        "10": {
            "class_type": "SamplerCustomAdvanced",
            "inputs": {
                "noise": ["7", 0],
                "guider": ["6", 0],
                "sampler": ["13", 0],
                "sigmas": ["9", 0],
                "latent_image": ["8", 0]
            }
        },
        "11": {
            "class_type": "VAEDecode",
            "inputs": {
                "samples": ["10", 0],
                "vae": ["3", 0]
            }
        },
        "12": {
            "class_type": "SaveImage",
            "inputs": {
                "images": ["11", 0],
                "filename_prefix": filename_prefix
            }
        }
    })
}

// ─── MiniMax H3 still workflow ────────────────────────────────────────────────

/// Shortest clip MiniMax H3 accepts (its frame grid is 17k+5).
const H3_STILL_FRAMES: u32 = 5;

/// MiniMax H3 canvases step in 32 px; round down, never below 32.
fn snap_to_32(px: u32) -> u32 {
    (px / 32 * 32).max(32)
}

/// Canvas size the MiniMax H3 graph actually renders for a requested size.
fn h3_canvas(width: u32, height: u32) -> (u32, u32) {
    (snap_to_32(width), snap_to_32(height))
}

/// Build a MiniMax H3 graph that renders a still: the shortest text-to-video
/// clip, decoded with the video VAE, with frame 0 saved as a PNG. The audio
/// half of the AV latent is never decoded. `guidance` does not apply to this
/// sampler (BasicGuider carries no CFG), matching the agentbox H3 reference
/// workflow.
fn build_minimax_h3_still_workflow(req: &ImageGenRequest, seed: u64, filename_prefix: &str) -> Value {
    let (width, height) = h3_canvas(req.width, req.height);
    json!({
        "1": {
            "class_type": "UNETLoader",
            "inputs": {
                "unet_name": "minimax_h3_fl2va_pruned_int8_convrot.safetensors",
                "weight_dtype": "default"
            }
        },
        "2": {
            "class_type": "CLIPLoader",
            "inputs": {
                "clip_name": "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
                "type": "minimax",
                "device": "default"
            }
        },
        "3": {
            "class_type": "VAELoader",
            "inputs": {
                "vae_name": "minimax_h3_video_vae_fp16.safetensors"
            }
        },
        "4": {
            "class_type": "MiniMaxH3ImageToVideo",
            "inputs": {
                "clip": ["2", 0],
                "vae": ["3", 0],
                "prompt": req.prompt,
                "width": width,
                "height": height,
                "length": H3_STILL_FRAMES
            }
        },
        "5": {
            "class_type": "RandomNoise",
            "inputs": {
                "noise_seed": seed
            }
        },
        "6": {
            "class_type": "BasicGuider",
            "inputs": {
                "model": ["1", 0],
                "conditioning": ["4", 0]
            }
        },
        "7": {
            "class_type": "KSamplerSelect",
            "inputs": {
                "sampler_name": "res_multistep"
            }
        },
        "8": {
            "class_type": "BasicScheduler",
            "inputs": {
                "model": ["1", 0],
                "scheduler": "simple",
                "steps": req.steps,
                "denoise": 1.0
            }
        },
        "9": {
            "class_type": "SamplerCustomAdvanced",
            "inputs": {
                "noise": ["5", 0],
                "guider": ["6", 0],
                "sampler": ["7", 0],
                "sigmas": ["8", 0],
                "latent_image": ["4", 1]
            }
        },
        "10": {
            "class_type": "VAEDecode",
            "inputs": {
                "samples": ["9", 0],
                "vae": ["3", 0]
            }
        },
        "11": {
            "class_type": "ImageFromBatch",
            "inputs": {
                "image": ["10", 0],
                "batch_index": 0,
                "length": 1
            }
        },
        "12": {
            "class_type": "SaveImage",
            "inputs": {
                "images": ["11", 0],
                "filename_prefix": filename_prefix
            }
        }
    })
}

/// Build the workflow for the selected model, returning it with the canvas
/// size it will render (H3 snaps to 32 px; Flux2 uses the request as given).
fn build_workflow(
    model: ImageModel,
    req: &ImageGenRequest,
    seed: u64,
    filename_prefix: &str,
) -> (Value, u32, u32) {
    match model {
        ImageModel::MiniMaxH3 => {
            let (w, h) = h3_canvas(req.width, req.height);
            (build_minimax_h3_still_workflow(req, seed, filename_prefix), w, h)
        }
        ImageModel::Flux2 => (
            build_flux2_workflow(req, seed, filename_prefix),
            req.width,
            req.height,
        ),
    }
}

// ─── Native ComfyUI job runner ────────────────────────────────────────────────

/// Poll budget for `/history`: 60 × 5 s. A MiniMax H3 still takes ~60 s
/// including a cold model load on the estate GPU.
const HISTORY_POLLS: u32 = 60;
const HISTORY_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// A finished ComfyUI job: its prompt id, first saved image and its bytes.
struct ComfyJob {
    prompt_id: String,
    filename: String,
    bytes: bytes::Bytes,
}

/// First image `SaveImage` recorded in a `/history/{prompt_id}` entry.
fn first_saved_image(job: &Value) -> Option<(String, String)> {
    let outputs = job.get("outputs")?.as_object()?;
    outputs.values().find_map(|node_out| {
        let first = node_out.get("images")?.as_array()?.first()?;
        let filename = first.get("filename")?.as_str()?.to_string();
        let subfolder = first
            .get("subfolder")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Some((filename, subfolder))
    })
}

/// Submit `workflow` to ComfyUI's native API, poll `/history` until an image
/// is saved, and fetch its bytes. Errors come back as the HTTP response the
/// caller should return.
async fn run_comfyui_job(workflow: Value, client_id: &str) -> Result<ComfyJob, HttpResponse> {
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .unwrap_or_default();

    let submit_resp = client
        .post(format!("{}/prompt", comfyui_base()))
        .json(&json!({ "prompt": workflow, "client_id": client_id }))
        .send()
        .await
        .map_err(|e| {
            error!("ComfyUI submit failed: {}", e);
            HttpResponse::ServiceUnavailable().json(json!({
                "error": "ComfyUI unreachable",
                "details": e.to_string()
            }))
        })?;

    if !submit_resp.status().is_success() {
        let body = submit_resp.text().await.unwrap_or_default();
        error!("ComfyUI rejected workflow: {}", body);
        return Err(HttpResponse::BadRequest().json(json!({
            "error": "ComfyUI rejected workflow",
            "details": body
        })));
    }

    let submit_json: Value = submit_resp.json().await.map_err(|e| {
        HttpResponse::InternalServerError().json(json!({
            "error": "Failed to parse ComfyUI response",
            "details": e.to_string()
        }))
    })?;

    let prompt_id = submit_json
        .get("prompt_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            HttpResponse::InternalServerError().json(json!({
                "error": "No prompt_id in ComfyUI response",
                "raw": submit_json
            }))
        })?;

    info!("ComfyUI accepted job {} → prompt_id {}", client_id, prompt_id);

    let history_url = format!("{}/history/{}", comfyui_base(), prompt_id);
    let mut saved: Option<(String, String)> = None;
    for attempt in 0..HISTORY_POLLS {
        sleep(HISTORY_POLL_INTERVAL).await;
        let history = match client.get(&history_url).send().await {
            Ok(r) => r.json::<Value>().await.unwrap_or_default(),
            Err(e) => {
                warn!("History poll {}/{} failed: {}", attempt + 1, HISTORY_POLLS, e);
                continue;
            }
        };
        if let Some(found) = history.get(&prompt_id).and_then(first_saved_image) {
            saved = Some(found);
            break;
        }
    }

    let (filename, subfolder) = saved.ok_or_else(|| {
        HttpResponse::GatewayTimeout().json(json!({
            "error": "Timed out waiting for ComfyUI to finish",
            "prompt_id": prompt_id
        }))
    })?;

    info!("ComfyUI finished: {}", filename);

    let view_url = format!(
        "{}/view?filename={}&subfolder={}&type=output",
        comfyui_base(),
        urlencoding::encode(&filename),
        urlencoding::encode(&subfolder)
    );
    let resp = client.get(&view_url).send().await.map_err(|e| {
        HttpResponse::InternalServerError().json(json!({
            "error": "Failed to GET image from ComfyUI",
            "details": e.to_string()
        }))
    })?;
    let bytes = resp.bytes().await.map_err(|e| {
        HttpResponse::InternalServerError().json(json!({
            "error": "Failed to fetch image bytes",
            "details": e.to_string()
        }))
    })?;

    Ok(ComfyJob {
        prompt_id,
        filename,
        bytes,
    })
}

// ─── Helper: get authenticated user from request ──────────────────────────────

async fn get_user_npub(req: &HttpRequest, nostr_service: &NostrService) -> Option<String> {
    // Extract session token from Authorization header or cookie
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let token = token?;

    // Split "pubkey:token"
    let parts: Vec<&str> = token.splitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }

    let pubkey = parts[0];
    if nostr_service.validate_session(pubkey, parts[1]).await {
        Some(pubkey.to_string())
    } else {
        None
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/image-gen/submit
pub async fn submit_image_job(
    req: HttpRequest,
    body: web::Json<ImageGenRequest>,
    nostr_service: web::Data<NostrService>,
) -> HttpResponse {
    // Auth check
    let user_npub = match get_user_npub(&req, &nostr_service).await {
        Some(u) => u,
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "error": "Authentication required",
                "details": "Valid Nostr session required to submit image jobs"
            }));
        }
    };

    let seed: u64 = if body.seed < 0 {
        rand::random()
    } else {
        body.seed as u64
    };

    let job_id = Uuid::new_v4().to_string();
    let filename_prefix = format!("visionclaw/{}/{}", user_npub, job_id);
    let model = ImageModel::from_env();
    let (workflow, width, height) = build_workflow(model, &body, seed, &filename_prefix);

    info!(
        "Submitting image job {} ({:?}) for user {} to ComfyUI",
        job_id,
        model,
        &user_npub[..8]
    );

    let job = match run_comfyui_job(workflow, &job_id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let prompt_id = job.prompt_id;
    let filename = job.filename;
    let image_bytes = job.bytes;

    // PUT to Solid pod — path: /solid/{user}/images/{job_id}.png
    let pod_path = format!(
        "/api/solid/pods/{}/{}/{}.png",
        user_npub, body.pod_folder, job_id
    );
    let solid_url = format!(
        "{}{}",
        solid_base().trim_end_matches("/api/solid"),
        &pod_path
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_default();
    let pod_store_resp = client
        .put(&solid_url)
        .header("Content-Type", "image/png")
        .header(
            "Authorization",
            req.headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
        )
        .body(image_bytes)
        .send()
        .await;

    let pod_image_url = match pod_store_resp {
        Ok(r) if r.status().is_success() || r.status().as_u16() == 201 => {
            info!("Stored image in Solid pod: {}", pod_path);
            Some(pod_path)
        }
        Ok(r) => {
            warn!("Solid pod store returned {}: storing skipped", r.status());
            None
        }
        Err(e) => {
            warn!("Failed to store in Solid pod: {}", e);
            None
        }
    };

    HttpResponse::Ok().json(ImageGenResponse {
        job_id: prompt_id,
        status: "completed".to_string(),
        pod_image_url,
        comfyui_filename: Some(filename),
        width,
        height,
        seed,
        error: None,
    })
}

/// POST /api/image-gen/agent-submit
/// Same native ComfyUI pipeline as `/submit` (the former Salad wrapper on
/// :3000 is not part of the ComfyUI sidecar, so this path never reached it).
///
/// Used by MCP agents in the agentic-workstation container — no user Nostr session needed.
/// Images are stored in the embedded solid-pod-rs storage under the `user_npub` pod.
pub async fn agent_submit_image_job(
    req: HttpRequest,
    body: web::Json<AgentImageGenRequest>,
) -> HttpResponse {
    // Check agent key
    let provided = req
        .headers()
        .get("x-agent-key")
        .and_then(|v| v.to_str().ok());
    if !agent_key_authorised(provided) {
        return HttpResponse::Unauthorized().json(json!({
            "error": "Invalid or missing X-Agent-Key header"
        }));
    }

    let user_npub = body
        .user_npub
        .clone()
        .unwrap_or_else(|| "agent".to_string());
    let seed: u64 = if body.seed < 0 {
        rand::random()
    } else {
        body.seed as u64
    };
    let job_id = Uuid::new_v4().to_string();
    let filename_prefix = format!("visionclaw/{}/{}", user_npub, job_id);

    let params = ImageGenRequest {
        prompt: body.prompt.clone(),
        negative_prompt: None,
        width: body.width,
        height: body.height,
        steps: body.steps,
        guidance: body.guidance,
        seed: body.seed,
        pod_folder: body.pod_folder.clone(),
    };
    let model = ImageModel::from_env();
    let (workflow, width, height) = build_workflow(model, &params, seed, &filename_prefix);

    info!(
        "[agent] Submitting image job {} ({:?}) for npub {} to ComfyUI",
        job_id,
        model,
        &user_npub[..8.min(user_npub.len())]
    );

    let job = match run_comfyui_job(workflow, &job_id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let prompt_id = job.prompt_id;
    let comfyui_filename = Some(job.filename);
    let image_bytes = job.bytes;

    // Store in embedded solid-pod-rs
    let pod_image_url = try_store_in_pod(&image_bytes, &user_npub, &body.pod_folder, &job_id).await;

    HttpResponse::Ok().json(ImageGenResponse {
        job_id: prompt_id,
        status: "completed".to_string(),
        pod_image_url,
        comfyui_filename,
        width,
        height,
        seed,
        error: None,
    })
}

/// Store PNG bytes in the embedded solid-pod-rs storage.
/// Returns the pod-relative URL on success, None on any failure.
///
/// When `solid-pod-embed` is enabled, this writes directly to the in-process
/// Solid storage backend. No HTTP round-trip or NIP-98 signing needed.
#[cfg(feature = "solid-pod-embed")]
async fn try_store_in_pod(
    image_bytes: &[u8],
    user_npub: &str,
    pod_folder: &str,
    job_id: &str,
) -> Option<String> {
    let storage = match crate::handlers::solid_proxy_handler::get_global_storage() {
        Some(s) => s,
        None => {
            warn!("[agent] solid-pod-rs storage not initialised — skipping pod storage");
            return None;
        }
    };

    let resource_path = format!("/{}/{}/{}.png", user_npub, pod_folder, job_id);

    // Ensure the user's pod container exists before writing
    let container_path = format!("/{}/{}/", user_npub, pod_folder);
    if !storage.exists(&container_path).await.unwrap_or(false) {
        if let Err(e) = storage.create_container(&container_path).await {
            warn!(
                "[agent] Failed to create pod container {}: {}",
                container_path, e
            );
        }
    }

    match storage
        .put(
            &resource_path,
            bytes::Bytes::from(image_bytes.to_vec()),
            "image/png",
        )
        .await
    {
        Ok(_) => {
            info!("[agent] Stored image in pod: {}", resource_path);
            Some(format!("/solid{}", resource_path))
        }
        Err(e) => {
            warn!("[agent] Pod PUT failed: {}", e);
            None
        }
    }
}

#[cfg(not(feature = "solid-pod-embed"))]
async fn try_store_in_pod(
    _image_bytes: &[u8],
    _user_npub: &str,
    _pod_folder: &str,
    _job_id: &str,
) -> Option<String> {
    warn!("[agent] solid-pod-embed feature not enabled, cannot store image");
    None
}

/// GET /api/image-gen/status/{job_id}
pub async fn get_job_status(path: web::Path<String>) -> HttpResponse {
    let job_id = path.into_inner();
    let client = Client::new();
    let history_url = format!("{}/history/{}", comfyui_base(), job_id);

    match client.get(&history_url).send().await {
        Ok(r) => {
            let status = r.status();
            let body: Value = r.json().await.unwrap_or_default();
            if body.get(&job_id).is_some() {
                HttpResponse::Ok().json(JobStatusResponse {
                    job_id: job_id.clone(),
                    status: "completed".to_string(),
                    outputs: body.get(&job_id).cloned(),
                })
            } else {
                HttpResponse::Ok().json(JobStatusResponse {
                    job_id,
                    status: if status.is_success() {
                        "pending".to_string()
                    } else {
                        "unknown".to_string()
                    },
                    outputs: None,
                })
            }
        }
        Err(e) => HttpResponse::ServiceUnavailable().json(json!({
            "error": "ComfyUI unreachable",
            "details": e.to_string()
        })),
    }
}

/// GET /api/image-gen/health
pub async fn health() -> HttpResponse {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    match client
        .get(format!("{}/system_stats", comfyui_base()))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            let stats: Value = r.json().await.unwrap_or_default();
            HttpResponse::Ok().json(json!({
                "status": "ok",
                "comfyui": "reachable",
                "vram_free": stats.pointer("/devices/0/vram_free"),
                "vram_total": stats.pointer("/devices/0/vram_total")
            }))
        }
        Ok(r) => HttpResponse::Ok().json(json!({
            "status": "degraded",
            "comfyui": format!("HTTP {}", r.status())
        })),
        Err(e) => HttpResponse::Ok().json(json!({
            "status": "degraded",
            "comfyui": "unreachable",
            "error": e.to_string()
        })),
    }
}

// ─── Route registration ───────────────────────────────────────────────────────

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/image-gen")
            .route("/health", web::get().to(health))
            .route("/submit", web::post().to(submit_image_job))
            .route("/agent-submit", web::post().to(agent_submit_image_job))
            .route("/status/{job_id}", web::get().to(get_job_status)),
    );
}

#[cfg(test)]
mod agent_key_tests {
    use super::{check_agent_key, constant_time_eq};

    #[test]
    fn unset_key_fails_closed() {
        // ADR-2093: the pre-fix code substituted "changeme-agent-key" here, so an
        // unconfigured deployment accepted a publicly-known literal.
        assert!(!check_agent_key(None, Some("changeme-agent-key")));
        assert!(!check_agent_key(None, Some("anything")));
        assert!(!check_agent_key(None, None));
    }

    #[test]
    fn empty_key_fails_closed() {
        assert!(!check_agent_key(Some(""), Some("")));
        assert!(!check_agent_key(Some(""), Some("x")));
    }

    #[test]
    fn missing_header_fails_closed() {
        assert!(!check_agent_key(Some("real-key"), None));
    }

    #[test]
    fn exact_match_authorises_and_mismatch_does_not() {
        assert!(check_agent_key(Some("real-key"), Some("real-key")));
        assert!(!check_agent_key(Some("real-key"), Some("real-ke")));
        assert!(!check_agent_key(Some("real-key"), Some("real-keyy")));
        assert!(!check_agent_key(Some("real-key"), Some("REAL-KEY")));
    }

    #[test]
    fn constant_time_eq_matches_equality_semantics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }
}

#[cfg(test)]
mod workflow_tests {
    use super::*;

    fn request(width: u32, height: u32) -> ImageGenRequest {
        ImageGenRequest {
            prompt: "a glass knowledge graph".to_string(),
            negative_prompt: None,
            width,
            height,
            steps: 20,
            guidance: 3.5,
            seed: 7,
            pod_folder: "images".to_string(),
        }
    }

    #[test]
    fn model_defaults_to_minimax_h3() {
        assert_eq!(ImageModel::parse(None), ImageModel::MiniMaxH3);
        assert_eq!(ImageModel::parse(Some("")), ImageModel::MiniMaxH3);
        assert_eq!(ImageModel::parse(Some("sdxl")), ImageModel::MiniMaxH3);
        assert_eq!(ImageModel::parse(Some("minimax-h3")), ImageModel::MiniMaxH3);
    }

    #[test]
    fn flux2_is_selectable() {
        assert_eq!(ImageModel::parse(Some("flux2")), ImageModel::Flux2);
        assert_eq!(ImageModel::parse(Some(" FLUX-2 ")), ImageModel::Flux2);
    }

    #[test]
    fn h3_canvas_snaps_down_to_32_with_floor() {
        assert_eq!(h3_canvas(1024, 1024), (1024, 1024));
        assert_eq!(h3_canvas(1000, 770), (992, 768));
        assert_eq!(h3_canvas(10, 0), (32, 32));
    }

    #[test]
    fn h3_workflow_uses_installed_weights_and_saves_one_frame() {
        let wf = build_minimax_h3_still_workflow(&request(1000, 770), 42, "visionclaw/u/j");
        assert_eq!(
            wf["1"]["inputs"]["unet_name"],
            "minimax_h3_fl2va_pruned_int8_convrot.safetensors"
        );
        assert_eq!(
            wf["2"]["inputs"]["clip_name"],
            "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors"
        );
        assert_eq!(wf["2"]["inputs"]["type"], "minimax");
        assert_eq!(wf["3"]["inputs"]["vae_name"], "minimax_h3_video_vae_fp16.safetensors");
        assert_eq!(wf["4"]["inputs"]["prompt"], "a glass knowledge graph");
        assert_eq!(wf["4"]["inputs"]["width"], 992);
        assert_eq!(wf["4"]["inputs"]["height"], 768);
        assert_eq!(wf["4"]["inputs"]["length"], H3_STILL_FRAMES);
        assert_eq!(wf["5"]["inputs"]["noise_seed"], 42);
        assert_eq!(wf["8"]["inputs"]["steps"], 20);
        assert_eq!(wf["9"]["inputs"]["latent_image"], json!(["4", 1]));
        assert_eq!(wf["11"]["class_type"], "ImageFromBatch");
        assert_eq!(wf["11"]["inputs"]["length"], 1);
        assert_eq!(wf["12"]["class_type"], "SaveImage");
        assert_eq!(wf["12"]["inputs"]["images"], json!(["11", 0]));
        assert_eq!(wf["12"]["inputs"]["filename_prefix"], "visionclaw/u/j");
    }

    #[test]
    fn every_h3_link_points_at_an_existing_node() {
        let wf = build_minimax_h3_still_workflow(&request(1024, 1024), 1, "p");
        let nodes = wf.as_object().unwrap();
        for (id, node) in nodes {
            for (name, input) in node["inputs"].as_object().unwrap() {
                if let Some(link) = input.as_array() {
                    let target = link[0].as_str().unwrap();
                    assert!(nodes.contains_key(target), "node {id} input {name} -> missing {target}");
                }
            }
        }
    }

    #[test]
    fn first_saved_image_reads_history_entry() {
        // Shape recorded from the live H3 acceptance run, 2026-09-30.
        let job = json!({"outputs": {"12": {"images": [
            {"filename": "h3-still_00001_.png", "subfolder": "visionclaw/e1-acceptance", "type": "output"}
        ]}}});
        assert_eq!(
            first_saved_image(&job),
            Some(("h3-still_00001_.png".to_string(), "visionclaw/e1-acceptance".to_string()))
        );
        assert_eq!(first_saved_image(&json!({"outputs": {}})), None);
        assert_eq!(first_saved_image(&json!({"status": {}})), None);
    }

    #[test]
    fn build_workflow_reports_rendered_canvas() {
        let req = request(1000, 770);
        let (_, w, h) = build_workflow(ImageModel::MiniMaxH3, &req, 1, "p");
        assert_eq!((w, h), (992, 768));
        let (wf, w, h) = build_workflow(ImageModel::Flux2, &req, 1, "p");
        assert_eq!((w, h), (1000, 770));
        assert_eq!(wf["1"]["inputs"]["unet_name"], "flux2_dev_fp8mixed.safetensors");
    }
}
