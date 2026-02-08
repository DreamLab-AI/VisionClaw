use crate::config::AppFullSettings; 
use crate::models::metadata::Metadata;
use crate::services::file_service::ProcessedFile;
use log::{error, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::utils::time;

const MARKDOWN_DIR: &str = "/app/data/markdown";

#[derive(Debug, Serialize, Deserialize)]
struct PerplexityResponse {
    content: String,
    link: String,
}

#[derive(Debug, Serialize)]
struct QueryRequest {
    query: String,
    conversation_id: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
    top_p: f32,
    presence_penalty: f32,
    frequency_penalty: f32,
}

pub struct PerplexityService {
    client: Client,
    settings: Arc<RwLock<AppFullSettings>>, 
}

impl PerplexityService {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            settings: Arc::new(RwLock::new(AppFullSettings::default())),
        }
    }

    pub async fn new_with_settings(
        settings: Arc<RwLock<AppFullSettings>>,
    ) -> Result<Self, Box<dyn StdError + Send + Sync>> {

        let timeout_duration = {
            let settings_read = settings.read().await;

            settings_read
                .perplexity
                .as_ref()
                .and_then(|p| p.timeout)
                .unwrap_or(30)
        };

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_duration))
            .build()?;

        Ok(Self {
            client,
            settings: Arc::clone(&settings),
        })
    }

    /// Chat completion method that takes a vector of (role, content) tuples
    pub async fn chat_completion(
        &self,
        messages: Vec<(String, String)>,
    ) -> Result<String, Box<dyn StdError + Send + Sync>> {
        // Convert messages to a single query string
        let query = messages
            .iter()
            .map(|(role, content)| format!("{}: {}", role, content))
            .collect::<Vec<_>>()
            .join("\n");

        // Use the existing query method
        self.query(&query, "default-conversation").await
    }

    pub async fn query(
        &self,
        query: &str,
        conversation_id: &str,
    ) -> Result<String, Box<dyn StdError + Send + Sync>> {
        let settings_read = self.settings.read().await;

        
        let perplexity_config = match settings_read.perplexity.as_ref() {
            Some(p) => p,
            None => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Perplexity settings not configured",
                )))
            }
        };

        
        let api_url = perplexity_config
            .api_url
            .as_deref()
            .ok_or("Perplexity API URL not configured")?;
        let api_key = perplexity_config
            .api_key
            .as_deref()
            .ok_or("Perplexity API Key not configured")?;
        let model = perplexity_config
            .model
            .as_deref()
            .ok_or("Perplexity model not configured")?;

        info!("Sending query to Perplexity API: {}", api_url);

        
        let request = QueryRequest {
            query: query.to_string(),
            conversation_id: conversation_id.to_string(),
            model: model.to_string(),
            max_tokens: perplexity_config.max_tokens.unwrap_or(4096),
            temperature: perplexity_config.temperature.unwrap_or(0.5),
            top_p: perplexity_config.top_p.unwrap_or(0.9),
            presence_penalty: perplexity_config.presence_penalty.unwrap_or(0.0),
            frequency_penalty: perplexity_config.frequency_penalty.unwrap_or(0.0),
        };

        let response = self
            .client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!(
                "Perplexity API error: Status: {}, Error: {}",
                status, error_text
            );
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Perplexity API error: {}", error_text),
            )));
        }

        let perplexity_response: PerplexityResponse = response.json().await?;
        Ok(perplexity_response.content)
    }

    pub async fn process_file(
        &self,
        file_name: &str,
    ) -> Result<ProcessedFile, Box<dyn StdError + Send + Sync>> {
        let file_path = format!("{}/{}", MARKDOWN_DIR, file_name);
        if !Path::new(&file_path).exists() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", file_name),
            )));
        }

        let content = fs::read_to_string(&file_path)?;
        let settings_read = self.settings.read().await;

        
        let perplexity_config = match settings_read.perplexity.as_ref() {
            Some(p) => p,
            None => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Perplexity settings not configured",
                )))
            }
        };

        
        let api_url = perplexity_config
            .api_url
            .as_deref()
            .ok_or("Perplexity API URL not configured")?;
        let api_key = perplexity_config
            .api_key
            .as_deref()
            .ok_or("Perplexity API Key not configured")?;

        info!("Sending request to Perplexity API: {}", api_url);

        
        let response = self
            .client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&content)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!(
                "Perplexity API error: Status: {}, Error: {}",
                status, error_text
            );
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Perplexity API error: {}", error_text),
            )));
        }

        let perplexity_response: PerplexityResponse = response.json().await?;

        
        let metadata = Metadata {
            file_name: file_name.to_string(),
            file_size: perplexity_response.content.len(),
            node_id: "0".to_string(),
            node_size: 10.0,
            hyperlink_count: 0,
            sha1: String::new(),
            last_modified: time::now(),
            last_content_change: Some(time::now()),
            last_commit: None,
            change_count: Some(1),
            file_blob_sha: None,
            perplexity_link: perplexity_response.link,
            last_perplexity_process: Some(time::now()),
            topic_counts: HashMap::new(),
            // Ontology fields (not applicable for Perplexity responses)
            term_id: None,
            preferred_term: None,
            source_domain: None,
            ontology_status: None,
            owl_class: None,
            owl_physicality: None,
            owl_role: None,
            quality_score: None,
            authority_score: None,
            belongs_to_domain: Vec::new(),
            maturity: None,
            is_subclass_of: Vec::new(),
            definition: None,
        };

        Ok(ProcessedFile {
            file_name: file_name.to_string(),
            content: perplexity_response.content,
            is_public: true,
            metadata,
        })
    }
}
