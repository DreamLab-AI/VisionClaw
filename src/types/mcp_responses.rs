use crate::from_json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json:::Value;
use std:::fmt;


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpResponse<T> {
    Success(McpSuccessResponse<T>),
    Error(McpErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSuccessResponse<T> {
    pub id: Option<Value>,
    pub result: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpErrorResponse {
    pub id: Option<Value>,
    pub error: McpError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpContent {
    Text(McpTextContent),
    Object(McpObjectContent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTextContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(deserialize_with = "deserialize_json_string")]
    pub text: Value, 
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpObjectContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub data: Value,
}

fn deserialize_json_string<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    from_json(&s).map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpContentResult {
    pub content: Vec<McpContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListResponse {
    pub agents: Vec<crate::services::bots_client::Agent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHistoryEntry {
    pub id: u64,
    pub server_id: String,
    pub method: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub success: bool,
    pub error_message: Option<String>,
}

// Re-export for convenience
pub type McpAgentResponse = McpResponse<McpContentResult>;

#[derive(Debug)]
pub enum McpParseError {
    JsonError(serde_json::Error),
    MissingContent,
    MissingTextField,
    InvalidStructure(String),
}

impl fmt::Display for McpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McpParseError::JsonError(e) => write!(f, "JSON parsing error: {}", e),
            McpParseError::MissingContent => write!(f, "Missing content array in MCP response"),
            McpParseError::MissingTextField => write!(f, "Missing text field in MCP content"),
            McpParseError::InvalidStructure(msg) => write!(f, "Invalid MCP structure: {}", msg),
        }
    }
}

impl std::error::Error for McpParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            McpParseError::JsonError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for McpParseError {
    fn from(err: serde_json::Error) -> Self {
        McpParseError::JsonError(err)
    }
}

impl<T> McpResponse<T> {
    
    pub fn into_result(self) -> Result<T, McpError> {
        match self {
            McpResponse::Success(success) => Ok(success.result),
            McpResponse::Error(error) => Err(error.error),
        }
    }

    
    pub fn is_success(&self) -> bool {
        matches!(self, McpResponse::Success(_))
    }

    
    pub fn is_error(&self) -> bool {
        matches!(self, McpResponse::Error(_))
    }
}

impl McpContentResult {
    
    pub fn extract_data<T>(&self) -> Result<T, McpParseError>
    where
        T: for<'a> Deserialize<'a>,
    {
        let first_content = self.content.first().ok_or(McpParseError::MissingContent)?;

        match first_content {
            McpContent::Text(text_content) => {
                serde_json::from_value(text_content.text.clone()).map_err(McpParseError::from)
            }
            McpContent::Object(obj_content) => {
                serde_json::from_value(obj_content.data.clone()).map_err(McpParseError::from)
            }
        }
    }

    
    pub fn extract_all_data<T>(&self) -> Result<Vec<T>, McpParseError>
    where
        T: for<'a> Deserialize<'a>,
    {
        let mut results = Vec::new();

        for content in &self.content {
            let data = match content {
                McpContent::Text(text_content) => {
                    serde_json::from_value(text_content.text.clone())?
                }
                McpContent::Object(obj_content) => {
                    serde_json::from_value(obj_content.data.clone())?
                }
            };
            results.push(data);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json:::json;
use crate::utils::json::{from_json, to_json};

    #[test]
    fn test_mcp_text_content_parsing() {
        let json = json!({
            "type": "text",
            "text": "{\"agents\": [{\"id\": \"1\", \"name\": \"test\", \"type\": \"test\", \"status\": \"active\"}]}"
        });

        let content: McpTextContent = serde_json::from_value(json).unwrap();
        let agents_data = content.text.get("agents").expect("Missing required key: agents");
        assert!(agents_data.is_array());
    }

    #[test]
    fn test_mcp_response_parsing() {
        let json = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"agents\": []}"
                }]
            }
        });

        let response: McpResponse<McpContentResult> = serde_json::from_value(json).unwrap();
        assert!(response.is_success());

        if let McpResponse::Success(success) = response {
            let agent_list: AgentListResponse = success.result.extract_data().unwrap();
            assert!(agent_list.agents.is_empty());
        }
    }
}

