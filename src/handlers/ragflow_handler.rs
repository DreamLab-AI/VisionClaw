use crate::handlers::validation_handler::ValidationService;
use crate::models::ragflow_chat::{RagflowChatRequest, RagflowChatResponse};
use crate::services::ragflow_service::ChatResponse;
use crate::types::speech::SpeechOptions;
use crate::utils::validation::errors::DetailedValidationError;
use crate::utils::validation::rate_limit::{extract_client_id, EndpointRateLimits, RateLimiter};
use crate::utils::validation::sanitization::Sanitizer;
use crate::utils::validation::MAX_REQUEST_SIZE;
use crate::AppState;
use actix_web::web::Bytes;
use actix_web::web::ServiceConfig;
use actix_web::HttpRequest;
use actix_web::{web, HttpResponse, Responder, Result};
use futures::StreamExt;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use crate::{
    ok_json, error_json,
    too_many_requests, service_unavailable,
};


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub user_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionResponse {
    pub success: bool,
    pub session_id: String,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub question: String,
    pub stream: Option<bool>,
    pub session_id: Option<String>,
    pub enable_tts: Option<bool>,
}

pub async fn send_message(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<SendMessageRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let ragflow_service = match &state.ragflow_service {
        Some(service) => service,
        None => {
            return service_unavailable!("RAGFlow service is not available")
        }
    };

    
    let session_id = match &request.session_id {
        Some(id) => id.clone(),
        None => state.ragflow_session_id.clone(),
    };

    let enable_tts = request.enable_tts.unwrap_or(false);
    
    match ragflow_service
        .send_message(
            session_id,
            request.question.clone(),
            false, 
            None,  
            request.stream.unwrap_or(true),
        )
        .await
    {
        Ok(response_stream) => {
            
            if enable_tts {
                if let Some(speech_service) = &state.speech_service {
                    let speech_service = speech_service.clone();
                    
                    let question = request.question.clone();
                    
                    actix_web::rt::spawn(async move {
                        let speech_options = SpeechOptions::default();
                        
                        if let Err(e) = speech_service
                            .text_to_speech(question, speech_options)
                            .await
                        {
                            error!("Error processing TTS: {:?}", e);
                        }
                    });
                }
            }

            
            let enable_tts = enable_tts; 
            let mapped_stream = response_stream.map(move |result| {
                result
                    .map(|answer| {
                        
                        if answer.is_empty() {
                            return Bytes::new();
                        }

                        
                        if enable_tts {
                            if let Some(speech_service) = &state.speech_service {
                                let speech_service = speech_service.clone();
                                let speech_options = SpeechOptions::default();
                                let answer_clone = answer.clone();
                                actix_web::rt::spawn(async move {
                                    if let Err(e) = speech_service
                                        .text_to_speech(answer_clone, speech_options)
                                        .await
                                    {
                                        error!("Error processing TTS for answer: {:?}", e);
                                    }
                                });
                            }
                        }

                        let json_response = json!({
                            "answer": answer,
                            "success": true
                        });
                        Bytes::from(json_response.to_string())
                    })
                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
            });
            Ok::<HttpResponse, actix_web::Error>(HttpResponse::Ok().streaming(mapped_stream))
        }
        Err(e) => {
            error!("Error sending message: {}", e);
            error_json!("Failed to send message: {}", e)
        }
    }
}

pub async fn create_session(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<CreateSessionRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let user_id = request.user_id.clone();
    let ragflow_service = match &state.ragflow_service {
        Some(service) => service,
        None => {
            return service_unavailable!("RAGFlow service is not available")
        }
    };

    match ragflow_service.create_session(user_id.clone()).await {
        Ok(session_id) => {




            info!(
                "Created new RAGFlow session: {}. Note: session ID cannot be stored in shared AppState.",
                session_id
            );


            ok_json!(CreateSessionResponse {
                success: true,
                session_id,
                message: None,
            })
        }
        Err(e) => {
            error!("Failed to initialize chat: {}", e);
            error_json!("Failed to initialize chat: {}", e)
        }
    }
}

pub async fn get_session_history(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    session_id: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let ragflow_service = match &state.ragflow_service {
        Some(service) => service,
        None => {
            return service_unavailable!("RAGFlow service is not available")
        }
    };

    match ragflow_service
        .get_session_history(session_id.to_string())
        .await
    {
        Ok(history) => ok_json!(history),
        Err(e) => {
            error!("Failed to get session history: {}", e);
            error_json!("Failed to get chat history: {}", e)
        }
    }
}

#[allow(dead_code)]
async fn handle_ragflow_chat(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<RagflowChatRequest>,
) -> Result<impl Responder, actix_web::Error> {
    
    let pubkey = match req
        .headers()
        .get("X-Nostr-Pubkey")
        .and_then(|v| v.to_str().ok())
    {
        Some(pk) => pk.to_string(),
        None => {
            return Ok(HttpResponse::Unauthorized()
                .json(json!({"error": "Missing X-Nostr-Pubkey header"})))
        }
    };
    let token = match req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok().map(|s| s.trim_start_matches("Bearer ")))
    {
        Some(t) => t.to_string(),
        None => {
            return Ok(HttpResponse::Unauthorized()
                .json(json!({"error": "Missing Authorization token"})))
        }
    };

    if let Some(nostr_service) = &state.nostr_service {
        if !nostr_service.validate_session(&pubkey, &token).await {
            return Ok(HttpResponse::Unauthorized().json(json!({"error": "Invalid session token"})));
        }
        
        let has_ragflow_specific_access = state.has_feature_access(&pubkey, "ragflow");
        let is_power_user = state.is_power_user(&pubkey);

        if !is_power_user && !has_ragflow_specific_access {
            return Ok(HttpResponse::Forbidden().json(json!({"error": "This feature requires power user access or specific RAGFlow permission"})));
        }
    } else {
        
        
        error!(
            "Nostr service not available during chat handling for pubkey: {}",
            pubkey
        );
        return Ok(HttpResponse::InternalServerError()
            .json(json!({"error": "Nostr service not available"})));
    }

    info!(
        "[handle_ragflow_chat] Checking RAGFlow service availability. Is Some: {}",
        state.ragflow_service.is_some()
    ); 

    let ragflow_service = match &state.ragflow_service {
        Some(service) => service,
        None => {
            error!("[handle_ragflow_chat] RAGFlow service is None, returning 503.");
            return Ok(HttpResponse::ServiceUnavailable()
                .json(json!({"error": "RAGFlow service not available"})));
        }
    };

    info!("[handle_ragflow_chat] RAGFlow service is Some. Proceeding."); 

    let mut session_id = payload.session_id.clone();
    if session_id.is_none() {
        
        match ragflow_service.create_session(pubkey.clone()).await {
            Ok(new_sid) => {
                info!(
                    "Created new RAGFlow session {} for pubkey {}",
                    new_sid, pubkey
                );
                session_id = Some(new_sid);
            }
            Err(e) => {
                error!(
                    "Failed to create RAGFlow session for pubkey {}: {}",
                    pubkey, e
                );
                return Ok(HttpResponse::InternalServerError()
                    .json(json!({"error": format!("Failed to create RAGFlow session: {}", e)})));
            }
        }
    }

    let current_session_id = match session_id {
        Some(sid) => sid,
        None => {
            error!("[handle_ragflow_chat] Session ID unexpectedly None after initialization");
            return Ok(HttpResponse::InternalServerError()
                .json(json!({"error": "Session initialization failed unexpectedly"})));
        }
    };

    let stream_preference = payload.stream.unwrap_or(false);
    match ragflow_service
        .send_chat_message(
            current_session_id.clone(),
            payload.question.clone(),
            stream_preference,
        )
        .await
    {
        Ok(ChatResponse::Buffered {
            answer,
            session_id: final_session_id,
        }) => {
            ok_json!(RagflowChatResponse {
                answer,
                session_id: final_session_id,
            })
        }
        Ok(ChatResponse::Streaming(stream)) => {
            Ok(HttpResponse::Ok()
                .content_type("text/event-stream")
                .streaming(stream))
        }
        Err(e) => {
            error!(
                "Error communicating with RAGFlow for session {}: {}",
                current_session_id, e
            );
            error_json!(json!({"error": format!("RAGFlow communication error: {}", e)}))
        }
    }
}
pub struct EnhancedRagFlowHandler {
    validation_service: ValidationService,
    rate_limiter: Arc<RateLimiter>,
}

impl EnhancedRagFlowHandler {
    pub fn new() -> Self {
        let config = EndpointRateLimits::ragflow_chat();
        let rate_limiter = Arc::new(RateLimiter::new(config));

        Self {
            validation_service: ValidationService::new(),
            rate_limiter,
        }
    }

    
    pub async fn chat_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
        payload: web::Json<Value>,
    ) -> Result<HttpResponse> {
        let client_id = extract_client_id(&req);

        
        if !self.rate_limiter.is_allowed(&client_id) {
            warn!(
                "Rate limit exceeded for RAGFlow chat from client: {}",
                client_id
            );
            return Ok(HttpResponse::TooManyRequests().json(json!({
                "error": "rate_limit_exceeded",
                "message": "Too many chat requests. Please wait before sending another message.",
                "retry_after": self.rate_limiter.reset_time(&client_id).as_secs()
            })));
        }

        
        let payload_size = serde_json::to_vec(&*payload).unwrap_or_default().len();
        if payload_size > MAX_REQUEST_SIZE {
            error!("RAGFlow chat payload too large: {} bytes", payload_size);
            return Ok(HttpResponse::PayloadTooLarge().json(json!({
                "error": "payload_too_large",
                "message": "Chat message too long",
                "max_size": MAX_REQUEST_SIZE
            })));
        }

        info!(
            "Processing enhanced RAGFlow chat from client: {} (size: {} bytes)",
            client_id, payload_size
        );

        
        let pubkey = match req
            .headers()
            .get("X-Nostr-Pubkey")
            .and_then(|v| v.to_str().ok())
        {
            Some(pk) => pk.to_string(),
            None => {
                warn!("Missing authentication header from client: {}", client_id);
                return Ok(HttpResponse::Unauthorized().json(json!({
                    "error": "authentication_required",
                    "message": "X-Nostr-Pubkey header is required"
                })));
            }
        };

        let token = match req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_start_matches("Bearer "))
        {
            Some(t) => t.to_string(),
            None => {
                warn!("Missing authorization token from client: {}", client_id);
                return Ok(HttpResponse::Unauthorized().json(json!({
                    "error": "authorization_required",
                    "message": "Authorization token is required"
                })));
            }
        };

        
        if let Some(nostr_service) = &state.nostr_service {
            if !nostr_service.validate_session(&pubkey, &token).await {
                warn!(
                    "Invalid session for pubkey: {} from client: {}",
                    pubkey, client_id
                );
                return Ok(HttpResponse::Unauthorized().json(json!({
                    "error": "invalid_session",
                    "message": "Invalid session token"
                })));
            }

            let has_ragflow_access = state.has_feature_access(&pubkey, "ragflow");
            let is_power_user = state.is_power_user(&pubkey);

            if !is_power_user && !has_ragflow_access {
                warn!(
                    "Insufficient permissions for pubkey: {} from client: {}",
                    pubkey, client_id
                );
                return Ok(HttpResponse::Forbidden().json(json!({
                    "error": "insufficient_permissions",
                    "message": "RAGFlow access requires power user privileges or specific permission"
                })));
            }
        } else {
            error!("Nostr service not available for authentication");
            return service_unavailable!("Authentication service is not available");
        }

        
        let validated_payload = match self.validation_service.validate_ragflow_chat(&payload) {
            Ok(sanitized) => sanitized,
            Err(validation_error) => {
                warn!(
                    "RAGFlow chat validation failed for client {}: {}",
                    client_id, validation_error
                );
                return Ok(validation_error.to_http_response());
            }
        };

        debug!("RAGFlow chat validation passed for client: {}", client_id);

        
        let question = validated_payload
            .get("question")
            .and_then(|q| q.as_str())
            .ok_or_else(|| {
                error!("Question field missing from validated payload");
                DetailedValidationError::missing_required_field("question")
            })?;

        let session_id = validated_payload
            .get("session_id")
            .and_then(|s| s.as_str())
            .map(String::from);

        let stream = validated_payload
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        let enable_tts = validated_payload
            .get("enable_tts")
            .and_then(|t| t.as_bool())
            .unwrap_or(false);

        
        self.validate_question_content(question)?;

        
        let ragflow_service = match &state.ragflow_service {
            Some(service) => service,
            None => {
                error!("RAGFlow service not available");
                return service_unavailable!("RAGFlow service is currently not available");
            }
        };

        let current_session_id = match session_id {
            Some(id) => id,
            None => {
                debug!("Creating new RAGFlow session for pubkey: {}", pubkey);
                match ragflow_service.create_session(pubkey.clone()).await {
                    Ok(new_session_id) => {
                        info!(
                            "Created new RAGFlow session {} for pubkey {}",
                            new_session_id, pubkey
                        );
                        new_session_id
                    }
                    Err(e) => {
                        error!("Failed to create RAGFlow session: {}", e);
                        return error_json!("session_creation_failed");
                    }
                }
            }
        };

        
        if enable_tts {
            self.process_tts_request(&state, question).await;
        }

        match ragflow_service
            .send_chat_message(current_session_id.clone(), question.to_string(), stream)
            .await
        {
            Ok(ChatResponse::Buffered {
                answer,
                session_id: final_session_id,
            }) => {
                info!(
                    "RAGFlow response received for client: {} (session: {})",
                    client_id, final_session_id
                );

                if enable_tts {
                    self.process_tts_request(&state, &answer).await;
                }

                ok_json!(RagflowChatResponse {
                    answer,
                    session_id: final_session_id,
                })
            }
            Ok(ChatResponse::Streaming(stream)) => {
                info!(
                    "RAGFlow streaming response started for client: {} (session: {})",
                    client_id, current_session_id
                );
                Ok(HttpResponse::Ok()
                    .content_type("text/event-stream")
                    .streaming(stream))
            }
            Err(e) => {
                error!(
                    "RAGFlow communication error for session {}: {}",
                    current_session_id, e
                );
                error_json!("ragflow_communication_failed")
            }
        }
    }

    
    pub async fn create_session_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
        payload: web::Json<Value>,
    ) -> Result<HttpResponse> {
        let client_id = extract_client_id(&req);

        
        if !self.rate_limiter.is_allowed(&client_id) {
            return too_many_requests!("Too many session creation requests");
        }

        info!(
            "Processing enhanced session creation from client: {}",
            client_id
        );

        
        let user_id = payload
            .get("user_id")
            .and_then(|u| u.as_str())
            .ok_or_else(|| DetailedValidationError::missing_required_field("user_id"))?;

        
        let sanitized_user_id = Sanitizer::sanitize_string(user_id).map_err(|e| {
            warn!("User ID sanitization failed: {}", e);
            e
        })?;

        let ragflow_service = match &state.ragflow_service {
            Some(service) => service,
            None => {
                return service_unavailable!("RAGFlow service is not available");
            }
        };

        match ragflow_service
            .create_session(sanitized_user_id.clone())
            .await
        {
            Ok(session_id) => {
                info!(
                    "RAGFlow session created: {} for user: {} (client: {})",
                    session_id, sanitized_user_id, client_id
                );
                ok_json!(json!({
                    "success": true,
                    "session_id": session_id,
                    "user_id": sanitized_user_id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            Err(e) => {
                error!(
                    "Failed to create RAGFlow session for user {}: {}",
                    sanitized_user_id, e
                );
                error_json!("session_creation_failed")
            }
        }
    }

    
    pub async fn get_session_history_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
        session_id: web::Path<String>,
    ) -> Result<HttpResponse> {
        let client_id = extract_client_id(&req);

        
        let history_rate_limiter = Arc::new(RateLimiter::new(
            crate::utils::validation::rate_limit::RateLimitConfig {
                requests_per_minute: 30,
                burst_size: 10,
                ..Default::default()
            },
        ));

        if !history_rate_limiter.is_allowed(&client_id) {
            return too_many_requests!("Too many history requests");
        }

        
        let sanitized_session_id = Sanitizer::sanitize_string(&session_id).map_err(|e| {
            warn!("Session ID sanitization failed: {}", e);
            e
        })?;

        debug!(
            "Getting session history for session: {} (client: {})",
            sanitized_session_id, client_id
        );

        let ragflow_service = match &state.ragflow_service {
            Some(service) => service,
            None => {
                return service_unavailable!("RAGFlow service is not available");
            }
        };

        match ragflow_service
            .get_session_history(sanitized_session_id.clone())
            .await
        {
            Ok(history) => {
                debug!(
                    "Session history retrieved for session: {}",
                    sanitized_session_id
                );
                ok_json!(json!({
                    "session_id": sanitized_session_id,
                    "history": history,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            Err(e) => {
                error!(
                    "Failed to get session history for {}: {}",
                    sanitized_session_id, e
                );
                error_json!("history_retrieval_failed")
            }
        }
    }

    
    fn validate_question_content(&self, question: &str) -> Result<(), DetailedValidationError> {
        
        let injection_patterns = [
            "ignore previous instructions",
            "forget everything above",
            "new instructions:",
            "system:",
            "\\n\\nUser:",
            "\\n\\nAssistant:",
            "<|im_star|>", 
            "<|im_en|>",   
        ];

        let question_lower = question.to_lowercase();
        for pattern in &injection_patterns {
            if question_lower.contains(pattern) {
                warn!("Potential prompt injection detected: {}", pattern);
                return Err(DetailedValidationError::malicious_content(
                    "question",
                    "prompt_injection",
                ));
            }
        }

        
        if self.has_excessive_repetition(question) {
            return Err(DetailedValidationError::new(
                "question",
                "Question contains excessive repetition",
                "EXCESSIVE_REPETITION",
            ));
        }

        
        if question.len() > 8000 {
            return Err(DetailedValidationError::new(
                "question",
                "Question is too long",
                "QUESTION_TOO_LONG",
            ));
        }

        Ok(())
    }

    
    fn has_excessive_repetition(&self, text: &str) -> bool {
        if text.len() < 50 {
            return false;
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        let mut word_counts = std::collections::HashMap::new();

        for word in &words {
            *word_counts.entry(word.to_lowercase()).or_insert(0) += 1;
        }

        
        let total_words = words.len();
        word_counts
            .values()
            .any(|&count| count as f64 / total_words as f64 > 0.3)
    }

    
    async fn process_tts_request(&self, state: &web::Data<AppState>, text: &str) {
        if let Some(speech_service) = &state.speech_service {
            let speech_service = speech_service.clone();
            let text = text.to_string();

            tokio::spawn(async move {
                if let Err(e) = speech_service
                    .text_to_speech(text, Default::default())
                    .await
                {
                    error!("TTS processing failed: {}", e);
                }
            });
        }
    }
}

impl Default for EnhancedRagFlowHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn config(cfg: &mut ServiceConfig) {
    let handler = web::Data::new(EnhancedRagFlowHandler::new());

    cfg.app_data(handler.clone())
        .service(
            web::scope("/ragflow")
                .route("/session", web::post().to(create_session)) 
                .route("/message", web::post().to(send_message))   
                .route("/chat", web::post().to(|req: HttpRequest, state: web::Data<AppState>, payload: web::Json<serde_json::Value>, handler: web::Data<EnhancedRagFlowHandler>| async move {

                    handler.chat_enhanced(req, state, payload).await
                })) 
                .route("/session/enhanced", web::post().to(|req, state, payload, handler: web::Data<EnhancedRagFlowHandler>| async move {
                    handler.create_session_enhanced(req, state, payload).await
                })) 
                .route("/history/{session_id}", web::get().to(get_session_history)) 
                .route("/history/enhanced/{session_id}", web::get().to(|req, state, session_id, handler: web::Data<EnhancedRagFlowHandler>| async move {
                    handler.get_session_history_enhanced(req, state, session_id).await
                })) 
        );
}
