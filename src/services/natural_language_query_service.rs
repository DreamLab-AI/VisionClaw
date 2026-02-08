//! Natural Language Query Service
//!
//! Translates natural language queries to Cypher using LLM and schema context

use crate::services::schema_service::SchemaService;
use crate::services::perplexity_service::PerplexityService;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Natural language to Cypher translation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTranslation {
    /// Original natural language query
    pub original_query: String,
    /// Generated Cypher query
    pub cypher_query: String,
    /// Explanation of what the query does
    pub explanation: String,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Any warnings or limitations
    pub warnings: Vec<String>,
}

/// Natural language query service
pub struct NaturalLanguageQueryService {
    schema_service: Arc<SchemaService>,
    perplexity_service: Arc<PerplexityService>,
}

impl NaturalLanguageQueryService {
    /// Create a new natural language query service
    pub fn new(
        schema_service: Arc<SchemaService>,
        perplexity_service: Arc<PerplexityService>,
    ) -> Self {
        Self {
            schema_service,
            perplexity_service,
        }
    }

    /// Translate natural language query to Cypher
    pub async fn translate_to_cypher(&self, query: &str) -> Result<QueryTranslation, String> {
        info!("Translating natural language query: {}", query);

        // Get schema context
        let schema_context = self.schema_service.get_llm_context().await;

        // Build LLM prompt
        let prompt = self.build_translation_prompt(query, &schema_context);

        // Call LLM service
        let response = self.perplexity_service
            .chat_completion(vec![
                ("system".to_string(), self.get_system_prompt()),
                ("user".to_string(), prompt),
            ])
            .await
            .map_err(|e| format!("LLM service error: {}", e))?;

        // Parse response
        self.parse_llm_response(query, &response)
    }

    /// Get multiple query suggestions for ambiguous input
    pub async fn suggest_queries(&self, query: &str) -> Result<Vec<QueryTranslation>, String> {
        info!("Generating query suggestions for: {}", query);

        let schema_context = self.schema_service.get_llm_context().await;
        let prompt = format!(
            "{}\n\nUser query: \"{}\"\n\nGenerate 3 different Cypher query interpretations.",
            schema_context, query
        );

        let response = self.perplexity_service
            .chat_completion(vec![
                ("system".to_string(), self.get_system_prompt()),
                ("user".to_string(), prompt),
            ])
            .await
            .map_err(|e| format!("LLM service error: {}", e))?;

        self.parse_multiple_queries(query, &response)
    }

    /// Validate Cypher query syntax
    pub fn validate_cypher(&self, cypher: &str) -> Result<(), String> {
        // Basic syntax validation
        let cypher_lower = cypher.to_lowercase();

        // Check for required MATCH or CREATE
        if !cypher_lower.contains("match") && !cypher_lower.contains("create") {
            return Err("Query must contain MATCH or CREATE clause".to_string());
        }

        // Check for RETURN clause (unless it's a CREATE/SET only query)
        if cypher_lower.contains("match") && !cypher_lower.contains("return") {
            return Err("MATCH queries must have RETURN clause".to_string());
        }

        // Check for dangerous operations
        if cypher_lower.contains("delete all") || cypher_lower.contains("drop") {
            return Err("Destructive operations not allowed".to_string());
        }

        Ok(())
    }

    /// Explain what a Cypher query does in natural language
    pub async fn explain_cypher(&self, cypher: &str) -> Result<String, String> {
        debug!("Explaining Cypher query");

        let prompt = format!(
            "Explain this Cypher query in simple terms:\n\n```cypher\n{}\n```",
            cypher
        );

        let response = self.perplexity_service
            .chat_completion(vec![
                ("system".to_string(), "You are a helpful assistant that explains graph database queries.".to_string()),
                ("user".to_string(), prompt),
            ])
            .await
            .map_err(|e| format!("LLM service error: {}", e))?;

        Ok(response)
    }

    // Private helper methods

    fn get_system_prompt(&self) -> String {
        r#"You are an expert Cypher query generator for Neo4j graph databases.

Your task is to translate natural language queries into valid Cypher queries.

Guidelines:
1. Always use GraphNode label for nodes
2. Always use EDGE label for relationships
3. Node properties: id, label, node_type, metadata_id, x, y, z, vx, vy, vz, mass, owl_class_iri
4. Relationship properties: weight, relation_type, owl_property_iri
5. Use parameterized queries when appropriate
6. Prefer MATCH over CREATE unless explicitly asked to create
7. Always include RETURN clause for queries
8. Use LIMIT to prevent large result sets
9. Be explicit about relationship directions

Response format:
```cypher
<query here>
```

Explanation: <brief explanation>

Confidence: <0.0-1.0>

Warnings: <any warnings or limitations>
"#.to_string()
    }

    fn build_translation_prompt(&self, query: &str, schema_context: &str) -> String {
        format!(
            "{}\n\nUser query: \"{}\"\n\nGenerate the appropriate Cypher query.",
            schema_context, query
        )
    }

    fn parse_llm_response(&self, original_query: &str, response: &str) -> Result<QueryTranslation, String> {
        // Extract Cypher query from response
        let cypher_query = self.extract_cypher_block(response)?;

        // Extract explanation
        let explanation = self.extract_after_marker(response, "Explanation:")
            .unwrap_or_else(|| "No explanation provided".to_string());

        // Extract confidence
        let confidence = self.extract_confidence(response).unwrap_or(0.5);

        // Extract warnings
        let warnings = self.extract_warnings(response);

        // Validate the generated Cypher
        if let Err(e) = self.validate_cypher(&cypher_query) {
            warn!("Generated invalid Cypher: {}", e);
            return Err(format!("Invalid Cypher generated: {}", e));
        }

        Ok(QueryTranslation {
            original_query: original_query.to_string(),
            cypher_query,
            explanation,
            confidence,
            warnings,
        })
    }

    fn parse_multiple_queries(&self, original_query: &str, response: &str) -> Result<Vec<QueryTranslation>, String> {
        // Split response by code blocks
        let mut translations = Vec::new();

        // Simple parsing - look for multiple ```cypher blocks
        let parts: Vec<&str> = response.split("```cypher").collect();

        for (i, part) in parts.iter().enumerate().skip(1) {
            if let Some(end_idx) = part.find("```") {
                let cypher = part[..end_idx].trim().to_string();

                if self.validate_cypher(&cypher).is_ok() {
                    translations.push(QueryTranslation {
                        original_query: original_query.to_string(),
                        cypher_query: cypher,
                        explanation: format!("Interpretation {}", i),
                        confidence: 0.5,
                        warnings: vec![],
                    });
                }
            }
        }

        if translations.is_empty() {
            return Err("No valid queries generated".to_string());
        }

        Ok(translations)
    }

    fn extract_cypher_block(&self, text: &str) -> Result<String, String> {
        // Look for ```cypher ... ``` block
        if let Some(start_idx) = text.find("```cypher") {
            let start = start_idx + "```cypher".len();
            if let Some(end_idx) = text[start..].find("```") {
                let cypher = text[start..start + end_idx].trim().to_string();
                return Ok(cypher);
            }
        }

        // Fallback: look for ```...``` block
        if let Some(start_idx) = text.find("```") {
            let start = start_idx + "```".len();
            if let Some(end_idx) = text[start..].find("```") {
                let cypher = text[start..start + end_idx].trim().to_string();
                return Ok(cypher);
            }
        }

        Err("No Cypher query found in response".to_string())
    }

    fn extract_after_marker(&self, text: &str, marker: &str) -> Option<String> {
        text.find(marker).map(|idx| {
            let start = idx + marker.len();
            text[start..]
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
    }

    fn extract_confidence(&self, text: &str) -> Option<f32> {
        if let Some(conf_str) = self.extract_after_marker(text, "Confidence:") {
            conf_str.parse::<f32>().ok()
        } else {
            None
        }
    }

    fn extract_warnings(&self, text: &str) -> Vec<String> {
        if let Some(warnings_str) = self.extract_after_marker(text, "Warnings:") {
            if warnings_str.to_lowercase() != "none" {
                return vec![warnings_str];
            }
        }
        vec![]
    }
}

/// Common natural language query patterns
pub struct QueryPatterns;

impl QueryPatterns {
    /// Get example queries for user guidance
    pub fn examples() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "Show me all person nodes",
                "MATCH (n:GraphNode {node_type: 'person'}) RETURN n LIMIT 50"
            ),
            (
                "Find all dependency relationships",
                "MATCH (a:GraphNode)-[r:EDGE {relation_type: 'dependency'}]->(b:GraphNode) RETURN a, r, b LIMIT 50"
            ),
            (
                "What are the direct children of Project X?",
                "MATCH (p:GraphNode {label: 'Project X'})-[r:EDGE {relation_type: 'hierarchy'}]->(c:GraphNode) RETURN c"
            ),
            (
                "Show me the shortest path between Node A and Node B",
                "MATCH path = shortestPath((a:GraphNode {label: 'Node A'})-[*]-(b:GraphNode {label: 'Node B'})) RETURN path"
            ),
            (
                "Find all nodes within 2 hops of Node X",
                "MATCH (start:GraphNode {label: 'Node X'})-[*1..2]-(connected:GraphNode) RETURN DISTINCT connected LIMIT 100"
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cypher_validation() {
        let service = create_test_service();

        // Valid query
        assert!(service.validate_cypher("MATCH (n:GraphNode) RETURN n").is_ok());

        // Missing RETURN
        assert!(service.validate_cypher("MATCH (n:GraphNode)").is_err());

        // Dangerous operation
        assert!(service.validate_cypher("MATCH (n) DELETE ALL").is_err());
    }

    #[test]
    fn test_extract_cypher_block() {
        let service = create_test_service();

        let response = r#"
Here's the query:

```cypher
MATCH (n:GraphNode) RETURN n
```

Explanation: This finds all nodes.
"#;

        let result = service.extract_cypher_block(response);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "MATCH (n:GraphNode) RETURN n");
    }

    #[test]
    fn test_query_patterns() {
        let examples = QueryPatterns::examples();
        assert!(!examples.is_empty());
        assert!(examples.len() >= 5);
    }

    fn create_test_service() -> NaturalLanguageQueryService {
        // Mock services for testing
        let schema_service = Arc::new(SchemaService::new());
        let perplexity_service = Arc::new(PerplexityService::new());
        NaturalLanguageQueryService::new(schema_service, perplexity_service)
    }
}
