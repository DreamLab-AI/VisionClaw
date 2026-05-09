use crate::config::AppFullSettings;
use crate::errors::{NetworkError, ParseError, VisionFlowError};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use log::{error, info};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::utils::json::to_json;

/// Helper to construct a `VisionFlowError::Network(HTTPError { .. })` from a status + message.
fn ragflow_status_error(status: StatusCode, msg: String) -> VisionFlowError {
    VisionFlowError::Network(NetworkError::HTTPError {
        url: "ragflow".to_string(),
        status: Some(status.as_u16()),
        reason: msg,
    })
}

/// Helper to construct a `VisionFlowError::Parse(JSON { .. })` from a message string.
fn ragflow_parse_error(msg: String) -> VisionFlowError {
    VisionFlowError::Parse(ParseError::JSON {
        input: "ragflow".to_string(),
        reason: msg,
    })
}

/// Result type for `send_chat_message`: either a buffered answer or a byte stream.
pub enum ChatResponse {
    /// Non-streaming: the complete answer and final session ID.
    Buffered { answer: String, session_id: String },
    /// Streaming: an SSE byte stream suitable for `HttpResponse::streaming()`.
    Streaming(Pin<Box<dyn Stream<Item = Result<Bytes, actix_web::Error>> + Send + 'static>>),
}

#[derive(Debug, Serialize)]
struct CompletionRequest {
    question: String,
    stream: bool,
    session_id: Option<String>,
    user_id: Option<String>,
    sync_dsl: Option<bool>,
}

pub struct RAGFlowService {
    client: Client,
    api_key: String,
    base_url: String,
    agent_id: String,
}

impl RAGFlowService {
    
    pub async fn new(_settings: Arc<RwLock<AppFullSettings>>) -> Result<Self, VisionFlowError> {
        
        let client = Client::new();
        

        info!("[RAGFlowService::new] Attempting to load RAGFlow config directly from environment variables.");

        let api_key = std::env::var("RAGFLOW_API_KEY").map_err(|e| {
            error!(
                "[RAGFlowService::new] Failed to read RAGFLOW_API_KEY: {}",
                e
            );
            ragflow_parse_error(format!(
                "RAGFLOW_API_KEY environment variable not found or invalid: {}",
                e
            ))
        })?;

        let base_url = std::env::var("RAGFLOW_API_BASE_URL").map_err(|e| {
            error!(
                "[RAGFlowService::new] Failed to read RAGFLOW_API_BASE_URL: {}",
                e
            );
            ragflow_parse_error(format!(
                "RAGFLOW_API_BASE_URL environment variable not found or invalid: {}",
                e
            ))
        })?;

        let agent_id = std::env::var("RAGFLOW_AGENT_ID").map_err(|e| {
            error!(
                "[RAGFlowService::new] Failed to read RAGFLOW_AGENT_ID: {}",
                e
            );
            ragflow_parse_error(format!(
                "RAGFLOW_AGENT_ID environment variable not found or invalid: {}",
                e
            ))
        })?;

        info!("[RAGFlowService::new] RAGFLOW_API_KEY: loaded (value redacted)");
        info!("[RAGFlowService::new] RAGFLOW_API_BASE_URL: {}", base_url);
        info!("[RAGFlowService::new] RAGFLOW_AGENT_ID: {}", agent_id);

        
        if api_key.is_empty() {
            error!(
                "[RAGFlowService::new] RAGFLOW_API_KEY is empty after loading from environment."
            );
            return Err(ragflow_parse_error(
                "RAGFLOW_API_KEY environment variable is empty".to_string(),
            ));
        }
        if base_url.is_empty() {
            error!("[RAGFlowService::new] RAGFLOW_API_BASE_URL is empty after loading from environment.");
            return Err(ragflow_parse_error(
                "RAGFLOW_API_BASE_URL environment variable is empty".to_string(),
            ));
        }
        if agent_id.is_empty() {
            error!(
                "[RAGFlowService::new] RAGFLOW_AGENT_ID is empty after loading from environment."
            );
            return Err(ragflow_parse_error(
                "RAGFLOW_AGENT_ID environment variable is empty".to_string(),
            ));
        }

        info!("[RAGFlowService::new] Successfully loaded RAGFlow API key, base URL, and agent ID from environment variables.");

        Ok(RAGFlowService {
            client,
            api_key,
            base_url,
            agent_id,
        })
    }

    pub async fn create_session(&self, user_id: String) -> Result<String, VisionFlowError> {
        info!("Creating session for user: {}", user_id);
        let url = format!(
            "{}/api/v1/agents/{}/sessions?user_id={}",
            self.base_url.trim_end_matches('/'),
            self.agent_id,
            user_id
        );
        info!("Full URL for create_session: {}", url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body("{}") 
            .send()
            .await?;

        let status = response.status();
        info!("Response status: {}", status);

        if status.is_success() {
            let result: serde_json::Value = response.json().await?;
            info!("Successful response: {:?}", result);

            
            match result["data"]["id"].as_str() {
                Some(id) => Ok(id.to_string()),
                None => {
                    error!("Failed to parse session ID from response: {:?}", result);
                    Err(ragflow_parse_error(
                        "Failed to parse session ID".to_string(),
                    ))
                }
            }
        } else {
            let error_message = response.text().await?;
            error!(
                "Failed to create session. Status: {}, Error: {}",
                status, error_message
            );
            Err(ragflow_status_error(status, error_message))
        }
    }

    pub async fn send_message(
        &self,
        session_id: String,
        message: String,
        _quote: bool,                  
        _doc_ids: Option<Vec<String>>, 
        stream: bool,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<String, VisionFlowError>> + Send + 'static>>,
        VisionFlowError,
    > {
        info!("Sending message to session: {}", session_id);
        let url = format!(
            "{}/api/v1/agents/{}/completions",
            self.base_url.trim_end_matches('/'),
            self.agent_id
        );
        info!("Full URL for send_message: {}", url);

        let request_body = CompletionRequest {
            question: message,
            stream,
            session_id: Some(session_id),
            user_id: None,
            sync_dsl: Some(false),
        };

        info!(
            "Request body: {:?}",
            to_json(&request_body).unwrap_or_default()
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        info!("Response status: {}", status);

        if status.is_success() {
            if stream {
                let stream = response.bytes_stream().map(move |chunk_result| {
                    match chunk_result {
                        Ok(chunk) => {
                            let chunk_str = String::from_utf8_lossy(&chunk);
                            
                            let chunk_str = chunk_str.trim();

                            if chunk_str.starts_with("data:") {
                                let json_str = chunk_str.trim_start_matches("data:").trim();
                                match serde_json::from_str::<serde_json::Value>(json_str) {
                                    Ok(json_response) => {
                                        if let Some(true) = json_response["data"].as_bool() {
                                            
                                            Ok("".to_string())
                                        } else if let Some(answer) =
                                            json_response["data"]["answer"].as_str()
                                        {
                                            Ok(answer.to_string())
                                        } else {
                                            Err(ragflow_parse_error(
                                                "No answer found in response".to_string(),
                                            ))
                                        }
                                    }
                                    Err(e) => Err(ragflow_parse_error(format!(
                                        "Failed to parse JSON: {}, content: {}",
                                        e, json_str
                                    ))),
                                }
                            } else {
                                Err(ragflow_parse_error(format!(
                                    "Invalid SSE format: {}",
                                    chunk_str
                                )))
                            }
                        }
                        Err(e) => Err(VisionFlowError::from(e)),
                    }
                });

                Ok(Box::pin(stream))
            } else {
                
                let result: serde_json::Value = response.json().await?;

                if let Some(answer) = result["data"]["answer"].as_str() {
                    
                    let stream = futures::stream::once(futures::future::ok(answer.to_string()));
                    Ok(Box::pin(stream))
                } else {
                    Err(ragflow_parse_error(
                        "No answer found in response".to_string(),
                    ))
                }
            }
        } else {
            let error_message = response.text().await?;
            error!(
                "Failed to send message. Status: {}, Error: {}",
                status, error_message
            );
            Err(ragflow_status_error(status, error_message))
        }
    }

    pub async fn get_session_history(
        &self,
        session_id: String,
    ) -> Result<serde_json::Value, VisionFlowError> {
        let url = format!(
            "{}/api/v1/agents/{}/sessions?id={}",
            self.base_url.trim_end_matches('/'),
            self.agent_id,
            session_id
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        let status = response.status();
        if status.is_success() {
            let history: serde_json::Value = response.json().await?;
            Ok(history)
        } else {
            let error_message = response.text().await?;
            error!(
                "Failed to get session history. Status: {}, Error: {}",
                status, error_message
            );
            Err(ragflow_status_error(status, error_message))
        }
    }

    pub async fn send_chat_message(
        &self,
        session_id: String,
        message: String,
        stream_preference: bool,
    ) -> Result<ChatResponse, VisionFlowError> {
        info!(
            "Sending chat message to RAGFlow session: {}, stream_preference: {}",
            session_id, stream_preference
        );
        let url = format!(
            "{}/api/v1/agents/{}/completions",
            self.base_url.trim_end_matches('/'),
            self.agent_id
        );

        let request_body = CompletionRequest {
            question: message,
            stream: stream_preference,
            session_id: Some(session_id.clone()),
            user_id: None,
            sync_dsl: Some(false),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_message = response.text().await?;
            error!(
                "RAGFlow chat API error. Status: {}, Error: {}",
                status, error_message
            );
            return Err(ragflow_status_error(status, error_message));
        }

        if !stream_preference {
            let result: serde_json::Value = response.json().await.map_err(|e| {
                ragflow_parse_error(format!(
                    "Failed to parse non-streamed JSON response: {}",
                    e
                ))
            })?;

            info!("RAGFlow non-streamed response: {:?}", result);

            let answer = result
                .get("data")
                .and_then(|data| data.get("answer"))
                .and_then(|ans| ans.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    ragflow_parse_error(
                        "Answer not found in non-streamed RAGFlow response".to_string(),
                    )
                })?;

            let final_session_id = result
                .get("data")
                .and_then(|data| data.get("session_id"))
                .and_then(|sid| sid.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| session_id.clone());

            Ok(ChatResponse::Buffered {
                answer,
                session_id: final_session_id,
            })
        } else {
            // True streaming: transform the upstream SSE byte stream into
            // parsed JSON-line chunks suitable for HttpResponse::streaming().
            let byte_stream = response.bytes_stream().map(move |chunk_result| {
                match chunk_result {
                    Ok(chunk_bytes) => {
                        let chunk_str = String::from_utf8_lossy(&chunk_bytes);
                        let mut out = String::new();

                        for line in chunk_str.lines() {
                            if !line.starts_with("data:") {
                                continue;
                            }
                            let json_str = line.trim_start_matches("data:").trim();
                            if json_str.is_empty() {
                                continue;
                            }

                            match serde_json::from_str::<serde_json::Value>(json_str) {
                                Ok(json_val) => {
                                    // Terminal sentinel — stream is done
                                    if json_val.get("code").and_then(|c| c.as_i64()) == Some(0)
                                        && json_val.get("data").and_then(|d| d.as_bool())
                                            == Some(true)
                                    {
                                        break;
                                    }

                                    if let Some(answer_chunk) = json_val
                                        .get("data")
                                        .and_then(|d| d.get("answer"))
                                        .and_then(|a| a.as_str())
                                    {
                                        out.push_str(answer_chunk);
                                    } else if let Some(answer_chunk) =
                                        json_val.get("answer").and_then(|a| a.as_str())
                                    {
                                        out.push_str(answer_chunk);
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to parse RAGFlow stream chunk JSON: {}. Chunk: '{}'",
                                        e,
                                        json_str
                                    );
                                }
                            }
                        }

                        Ok(Bytes::from(out))
                    }
                    Err(e) => {
                        log::error!("Error reading RAGFlow stream chunk: {}", e);
                        Err(actix_web::error::ErrorInternalServerError(format!(
                            "RAGFlow stream error: {}",
                            e
                        )))
                    }
                }
            });

            Ok(ChatResponse::Streaming(Box::pin(byte_stream)))
        }
    }
} 

impl Clone for RAGFlowService {
    fn clone(&self) -> Self {
        RAGFlowService {
            client: self.client.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            agent_id: self.agent_id.clone(),
        }
    }
}
