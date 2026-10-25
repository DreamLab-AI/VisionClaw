//! Natural Language Query Service
//!
//! Translates natural language queries to read-only SPARQL against the
//! embedded Oxigraph store using LLM and schema context. ADR-2063: the store
//! is Oxigraph (see `crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs`),
//! whose query language is SPARQL 1.1 — not Cypher, and not Neo4j. Generated
//! queries are validated read-only (SELECT/ASK/CONSTRUCT/DESCRIBE) via the
//! same validator the `/api/ontology/{query,sparql}` handlers enforce
//! ([`crate::handlers::ontology_handler::validate_read_only_sparql`]) before
//! being handed back to the caller; no query this service returns is executed
//! server-side, so the validator is the only gate standing between an LLM
//! hallucination and a caller pasting a mutating SPARQL string elsewhere.

use crate::services::perplexity_service::PerplexityService;
use crate::services::schema_service::SchemaService;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Natural language to SPARQL translation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTranslation {
    /// Original natural language query
    pub original_query: String,
    /// Generated read-only SPARQL query (SELECT/ASK/CONSTRUCT/DESCRIBE)
    pub sparql_query: String,
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
    /// Jev option (VC-25-S1-02). `None` ⇒ pre-change behaviour exactly.
    jev: Option<visionclaw_jev::JevHub>,
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
            jev: None,
        }
    }

    /// Attach the Jev option: bounded template planning is tried before the
    /// generative route. Every fallback returns to the generative route.
    pub fn with_jev(mut self, jev: visionclaw_jev::JevHub) -> Self {
        self.jev = Some(jev);
        self
    }

    /// Try the bounded template planner. `None` for every fallback (no client,
    /// flag off, no endpoint, error, low confidence, no grounded entity).
    async fn try_jev_plan(&self, query: &str) -> Option<QueryTranslation> {
        let jev = self.jev.as_ref()?;
        crate::services::jev_nlq::plan_query(jev, query).await.ok()
    }


    /// Translate natural language query to read-only SPARQL
    pub async fn translate_to_sparql(&self, query: &str) -> Result<QueryTranslation, String> {
        info!("Translating natural language query: {}", query);

        // VC-25-S1-02: bounded template fast path; None ⇒ unchanged generative route.
        if let Some(planned) = self.try_jev_plan(query).await {
            return Ok(planned);
        }

        // Get schema context
        let schema_context = self.schema_service.get_llm_context().await;

        // Build LLM prompt
        let prompt = self.build_translation_prompt(query, &schema_context);

        // Call LLM service
        let response = self
            .perplexity_service
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
            "{}\n\nUser query: \"{}\"\n\nGenerate 3 different read-only SPARQL query interpretations.",
            schema_context, query
        );

        let response = self
            .perplexity_service
            .chat_completion(vec![
                ("system".to_string(), self.get_system_prompt()),
                ("user".to_string(), prompt),
            ])
            .await
            .map_err(|e| format!("LLM service error: {}", e))?;

        self.parse_multiple_queries(query, &response)
    }

    /// Validate that a generated query is read-only SPARQL.
    ///
    /// Delegates to the same validator the `/api/ontology/{query,sparql}`
    /// handlers enforce (ADR-2063), so only SELECT/ASK/CONSTRUCT/DESCRIBE
    /// pass and any SPARQL Update form (INSERT/DELETE/DROP/CLEAR/LOAD/
    /// CREATE/ADD/MOVE/COPY/WITH/SERVICE) is rejected. One validator, one
    /// source of truth — this service does not maintain a second copy.
    pub fn validate_sparql(&self, sparql: &str) -> Result<(), String> {
        crate::handlers::ontology_handler::validate_read_only_sparql(sparql)
    }

    /// Explain what a SPARQL query does in natural language
    pub async fn explain_sparql(&self, sparql: &str) -> Result<String, String> {
        debug!("Explaining SPARQL query");

        let prompt = format!(
            "Explain this SPARQL query in simple terms:\n\n```sparql\n{}\n```",
            sparql
        );

        let response = self
            .perplexity_service
            .chat_completion(vec![
                (
                    "system".to_string(),
                    "You are a helpful assistant that explains graph database queries.".to_string(),
                ),
                ("user".to_string(), prompt),
            ])
            .await
            .map_err(|e| format!("LLM service error: {}", e))?;

        Ok(response)
    }

    // Private helper methods

    /// System prompt describing the *real* Oxigraph vocabulary (ADR-2063).
    /// Named graphs, class/predicate IRIs and prefixes below are the ones
    /// actually minted by `crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs`
    /// and `src/adapters/oxigraph_graph_repository.rs` — nothing here is invented.
    fn get_system_prompt(&self) -> String {
        r#"You are an expert SPARQL 1.1 query generator for an embedded Oxigraph RDF quad-store.

Your task is to translate natural language queries into valid, READ-ONLY SPARQL
(SELECT, ASK, CONSTRUCT or DESCRIBE only — never INSERT/DELETE/DROP/CLEAR/LOAD/
CREATE/ADD/MOVE/COPY/WITH/SERVICE).

Prefixes (always usable without a PREFIX line, but include them if you use one):
  vc:   <https://narrativegoldmine.com/ns/v1#>
  rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
  rdfs: <http://www.w3.org/2000/01/rdf-schema#>
  owl:  <http://www.w3.org/2002/07/owl#>
  xsd:  <http://www.w3.org/2001/XMLSchema#>

Named graphs (use `GRAPH <iri> { ... }` to scope a pattern to one of these):
  urn:ngm:graph:ontology:assert    - asserted OWL classes, properties, axioms (vc:OntologyClass)
  urn:ngm:graph:ontology:inferred  - Whelk-derived inferred SubClassOf axioms
  urn:ngm:graph:knowledge          - knowledge-graph nodes/edges (vc:KnowledgeNode, vc:KGEdge, vc:BridgeEdge)
  urn:ngm:graph:agent              - agent-swarm nodes (vc:Agent)

Vocabulary — knowledge graph (urn:ngm:graph:knowledge / urn:ngm:graph:agent):
  Node types (rdf:type / `a`): vc:KnowledgeNode, vc:Agent, vc:OntologyClass
  Node predicates: vc:nodeId (xsd:integer), rdfs:label, vc:nodeType, vc:metadataId,
    vc:hasX/vc:hasY/vc:hasZ (position, xsd:float), vc:velX/vc:velY/vc:velZ (velocity, xsd:float),
    vc:mass, vc:owlClass (IRI into the ontology graph), vc:meta ("key=value" strings)
  Edge types: vc:KGEdge, vc:BridgeEdge
  Edge predicates: vc:source, vc:target, vc:weight (xsd:float), vc:relationshipType, vc:owlProperty

Vocabulary — ontology (urn:ngm:graph:ontology:assert / :inferred):
  Class type: vc:OntologyClass (also owl:Class)
  Predicates: rdfs:label, rdfs:comment, rdfs:subClassOf, rdfs:domain, rdfs:range,
    vc:termId, vc:preferredTerm, vc:description, vc:sourceDomain, vc:classType,
    vc:status, vc:maturity, vc:qualityScore, vc:authorityScore, vc:owlPhysicality,
    vc:owlRole, vc:belongsToDomain, vc:bridgesToDomain, vc:hasPart, vc:isPartOf,
    vc:requires, vc:dependsOn, vc:enables, vc:relatesTo, vc:bridgesTo, vc:bridgesFrom

Guidelines:
1. Only generate SELECT, ASK, CONSTRUCT or DESCRIBE queries. Never generate
   INSERT/DELETE/DROP/CLEAR/LOAD/CREATE/ADD/MOVE/COPY/WITH/SERVICE — those are
   rejected by the server's read-only validator and will never execute.
2. Scope patterns to the correct named graph with `GRAPH <urn:ngm:graph:...> { }`
   when the query is about one specific graph (e.g. ontology classes vs. live
   knowledge-graph nodes); omit GRAPH to query across the default/union scope.
3. Always include a `LIMIT` clause (the server also clamps this, but state one).
4. Use `?var` bindings and real predicate IRIs from the vocabulary above — never
   invent a predicate or a Cypher-style label.
5. Be explicit about triple direction (subject predicate object).

Response format:
```sparql
<query here>
```

Explanation: <brief explanation>

Confidence: <0.0-1.0>

Warnings: <any warnings or limitations>
"#
        .to_string()
    }

    fn build_translation_prompt(&self, query: &str, schema_context: &str) -> String {
        format!(
            "{}\n\nUser query: \"{}\"\n\nGenerate the appropriate read-only SPARQL query.",
            schema_context, query
        )
    }

    fn parse_llm_response(
        &self,
        original_query: &str,
        response: &str,
    ) -> Result<QueryTranslation, String> {
        // Extract SPARQL query from response
        let sparql_query = self.extract_sparql_block(response)?;

        // Extract explanation
        let explanation = self
            .extract_after_marker(response, "Explanation:")
            .unwrap_or_else(|| "No explanation provided".to_string());

        // Extract confidence
        let confidence = self.extract_confidence(response).unwrap_or(0.5);

        // Extract warnings
        let warnings = self.extract_warnings(response);

        // Validate the generated SPARQL is read-only
        if let Err(e) = self.validate_sparql(&sparql_query) {
            warn!("Generated invalid SPARQL: {}", e);
            return Err(format!("Invalid SPARQL generated: {}", e));
        }

        Ok(QueryTranslation {
            original_query: original_query.to_string(),
            sparql_query,
            explanation,
            confidence,
            warnings,
        })
    }

    fn parse_multiple_queries(
        &self,
        original_query: &str,
        response: &str,
    ) -> Result<Vec<QueryTranslation>, String> {
        // Split response by code blocks
        let mut translations = Vec::new();

        // Simple parsing - look for multiple ```sparql blocks
        let parts: Vec<&str> = response.split("```sparql").collect();

        for (i, part) in parts.iter().enumerate().skip(1) {
            if let Some(end_idx) = part.find("```") {
                let sparql = part[..end_idx].trim().to_string();

                if self.validate_sparql(&sparql).is_ok() {
                    translations.push(QueryTranslation {
                        original_query: original_query.to_string(),
                        sparql_query: sparql,
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

    fn extract_sparql_block(&self, text: &str) -> Result<String, String> {
        // Look for ```sparql ... ``` block
        if let Some(start_idx) = text.find("```sparql") {
            let start = start_idx + "```sparql".len();
            if let Some(end_idx) = text[start..].find("```") {
                let sparql = text[start..start + end_idx].trim().to_string();
                return Ok(sparql);
            }
        }

        // Fallback: look for ```...``` block
        if let Some(start_idx) = text.find("```") {
            let start = start_idx + "```".len();
            if let Some(end_idx) = text[start..].find("```") {
                let sparql = text[start..start + end_idx].trim().to_string();
                return Ok(sparql);
            }
        }

        Err("No SPARQL query found in response".to_string())
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
    /// Get example queries for user guidance. Vocabulary matches the real
    /// Oxigraph store (ADR-2063): `vc:` predicates over the
    /// `urn:ngm:graph:knowledge` / `urn:ngm:graph:ontology:assert` named graphs.
    pub fn examples() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "Show me all knowledge-graph nodes",
                "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\nSELECT ?node ?label WHERE { GRAPH <urn:ngm:graph:knowledge> { ?node a vc:KnowledgeNode ; rdfs:label ?label } } LIMIT 50"
            ),
            (
                "Find all knowledge-graph edges and their weights",
                "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\nSELECT ?src ?tgt ?weight WHERE { GRAPH <urn:ngm:graph:knowledge> { ?edge a vc:KGEdge ; vc:source ?src ; vc:target ?tgt ; vc:weight ?weight } } LIMIT 50"
            ),
            (
                "What OWL classes belong to the physics domain?",
                "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\nSELECT ?class ?term WHERE { GRAPH <urn:ngm:graph:ontology:assert> { ?class a vc:OntologyClass ; vc:preferredTerm ?term ; vc:sourceDomain \"physics\" } } LIMIT 50"
            ),
            (
                "What are the subclasses of a given OWL class?",
                "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\nASK { GRAPH <urn:ngm:graph:ontology:inferred> { ?child rdfs:subClassOf <urn:ngm:class:example> } }"
            ),
            (
                "Describe the ontology class with a given IRI",
                "DESCRIBE <urn:ngm:class:example>"
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-2063: a SELECT query is accepted by the read-only validator.
    #[test]
    fn test_select_accepted() {
        let service = create_test_service();
        assert!(service
            .validate_sparql("SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10")
            .is_ok());
        assert!(service
            .validate_sparql("ASK { ?s a <urn:ngm:class:x> }")
            .is_ok());
    }

    /// ADR-2063: mutating SPARQL forms are rejected, not translated as if
    /// they were Cypher writes.
    #[test]
    fn test_mutating_sparql_rejected() {
        let service = create_test_service();
        assert!(service
            .validate_sparql("INSERT DATA { <urn:ngm:class:x> <urn:ngm:p> \"y\" }")
            .is_err());
        assert!(service
            .validate_sparql("DELETE WHERE { ?s ?p ?o }")
            .is_err());
        assert!(service
            .validate_sparql("DROP GRAPH <urn:ngm:graph:knowledge>")
            .is_err());
    }

    #[test]
    fn test_extract_sparql_block() {
        let service = create_test_service();

        let response = r#"
Here's the query:

```sparql
SELECT ?s WHERE { ?s a <urn:ngm:class:x> } LIMIT 10
```

Explanation: This finds all instances.
"#;

        let result = service.extract_sparql_block(response);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "SELECT ?s WHERE { ?s a <urn:ngm:class:x> } LIMIT 10"
        );
    }

    /// ADR-2063: an LLM response with no fenced query block is an error, not
    /// a silently empty/garbage query.
    #[test]
    fn test_extract_sparql_block_missing_is_error() {
        let service = create_test_service();
        let response = "I could not find an appropriate query for that request.";
        assert!(service.extract_sparql_block(response).is_err());
    }

    #[test]
    fn test_query_patterns() {
        let examples = QueryPatterns::examples();
        assert!(!examples.is_empty());
        assert!(examples.len() >= 5);
        for (_, query) in &examples {
            let upper = query.to_uppercase();
            assert!(
                upper.contains("SELECT") || upper.contains("ASK") || upper.contains("DESCRIBE"),
                "example query is not a read-only SPARQL form: {query}"
            );
        }
    }

    // ---- VC-25-S1-02: Jev option --------------------------------------

    fn jev_body(entity: bool) -> String {
        let mut a = serde_json::json!({
            "template": {"type":"choice","choice":"describe_class","confidence":0.97,"probabilities":{}},
            "answerable": {"type":"noul","noul":0.97},
        });
        if entity {
            a["entity"] = serde_json::json!({"type":"choice","choice":"e0","confidence":0.97,"probabilities":{}});
        }
        serde_json::json!({"model":"m","answers":a,"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }

    #[tokio::test]
    async fn jev_plan_none_for_every_fallback() {
        use visionclaw_jev::client::testing::{MockTransport, Script};
        use visionclaw_jev::hub::testing::enabled_hub;
        use visionclaw_jev::JevHub;
        let q = "describe urn:ngm:class:Physics";
        // No Jev attached.
        assert!(create_test_service().try_jev_plan(q).await.is_none());
        // Flag off.
        let off = create_test_service().with_jev(JevHub::disabled());
        assert!(off.try_jev_plan(q).await.is_none());
        // Error / malformed / unthresholded.
        for (script, auto) in [
            (Script::Reply(500, "".into()), Some("0.9")),
            (Script::Reply(200, "junk".into()), Some("0.9")),
            (Script::Reply(200, jev_body(true)), None),
        ] {
            let extra: Vec<(&str, &str)> = auto.map(|a| vec![("VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE", a)]).unwrap_or_default();
            let svc = create_test_service().with_jev(enabled_hub(&extra, MockTransport::new(script)));
            assert!(svc.try_jev_plan(q).await.is_none());
        }
    }

    #[tokio::test]
    async fn jev_plan_accept_path_validates_read_only() {
        use visionclaw_jev::client::testing::{MockTransport, Script};
        use visionclaw_jev::hub::testing::enabled_hub;
        let svc = create_test_service().with_jev(enabled_hub(
            &[("VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE", "0.9")],
            MockTransport::new(Script::Reply(200, jev_body(true))),
        ));
        let t = svc.try_jev_plan("describe urn:ngm:class:Physics").await.unwrap();
        assert_eq!(t.sparql_query, "DESCRIBE <urn:ngm:class:Physics>");
        assert!(svc.validate_sparql(&t.sparql_query).is_ok());
        // translate_to_sparql returns the planned query without reaching the LLM.
        let t2 = svc.translate_to_sparql("describe urn:ngm:class:Physics").await.unwrap();
        assert_eq!(t2.sparql_query, t.sparql_query);
    }

    fn create_test_service() -> NaturalLanguageQueryService {
        // Mock services for testing
        let schema_service = Arc::new(SchemaService::new());
        let perplexity_service = Arc::new(PerplexityService::new());
        NaturalLanguageQueryService::new(schema_service, perplexity_service)
    }
}
