//! `ontology-corpus.rvdb` and its `.generation.json` sidecar — the portable
//! vector bundle.
//!
//! **Division of labour.** Loom already owns the Postgres write channel:
//! `loom-scaffold`'s `build-concept-records` turns `scaffold-index.json` +
//! `prose-index.json` into concept records, and `loom-vector-ruvector`'s
//! `stage_corpus` embeds and stages them into `ruvector-postgres`. This module
//! does **not** replace either. It produces the *portable* form — records with
//! their vectors inline — for a consumer that wants the bundle without a
//! database, and it writes the sidecar Loom's generation law requires.
//!
//! **The embedder lock is not a convention.** `bge-small-en-v1.5` at 384
//! dimensions, and only that: the whole namespace must be cosine-comparable,
//! so every returned vector is length-checked and a mismatch is an error, not
//! a quietly wrong answer. The HNSW rebuild after a bulk load stays a separate,
//! non-concurrent step and is never folded in here.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use serde_json::{json, Value};

/// The locked embedding model.
pub const MODEL_ID: &str = "bge-small-en-v1.5";
/// The locked vector width.
pub const DIMENSIONS: usize = 384;
/// Default Xinference endpoint on the LAN.
pub const DEFAULT_ENDPOINT: &str = "http://192.168.2.132:9997/v1/embeddings";

/// One concept record: the seed-finding surface a semantic query matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptRecord {
    /// `urn:ngm:class:<slug>` — the join key across ttl, scaffold, prose, HNSW.
    pub key: String,
    /// `loom:ontology-corpus:<key>` — the primary key, so upserts are
    /// idempotent.
    pub id: String,
    /// The embedded text.
    pub text: String,
    /// slug / title / domain / maturity / quality / `has_prose` / generation.
    pub metadata: Value,
    /// The unit-norm vector, present only after embedding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

/// Anything that can turn texts into vectors.
pub trait Embedder {
    /// Embed a batch, returning one vector per input in order.
    ///
    /// # Errors
    /// Any transport or protocol failure, or a vector of the wrong width.
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// Xinference's OpenAI-compatible `/v1/embeddings` endpoint.
#[derive(Debug, Clone)]
pub struct Xinference {
    endpoint: String,
    timeout: std::time::Duration,
}

impl Xinference {
    /// Point at an endpoint.
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            timeout: std::time::Duration::from_secs(120),
        }
    }
}

impl Default for Xinference {
    fn default() -> Self {
        Self::new(DEFAULT_ENDPOINT)
    }
}

impl Embedder for Xinference {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let response: Value = ureq::AgentBuilder::new()
            .timeout(self.timeout)
            .build()
            .post(&self.endpoint)
            .send_json(json!({ "model": MODEL_ID, "input": texts }))
            .map_err(|e| anyhow::anyhow!("xinference request failed: {e}"))?
            .into_json()
            .map_err(|e| anyhow::anyhow!("xinference response was not json: {e}"))?;

        let data = response["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("xinference response has no `data` array"))?;
        anyhow::ensure!(
            data.len() == texts.len(),
            "xinference returned {} vectors for {} inputs",
            data.len(),
            texts.len()
        );

        data.iter()
            .map(|item| {
                let v: Vec<f32> = item["embedding"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("embedding is not an array"))?
                    .iter()
                    .map(|n| {
                        #[allow(clippy::cast_possible_truncation)]
                        let v = n.as_f64().unwrap_or(0.0) as f32;
                        v
                    })
                    .collect();
                anyhow::ensure!(
                    v.len() == DIMENSIONS,
                    "embedder returned {} dimensions, not {DIMENSIONS}: the namespace is \
                     locked to {MODEL_ID} and a mismatch silently invalidates the index",
                    v.len()
                );
                Ok(normalise(v))
            })
            .collect()
    }
}

/// Scale a vector to unit length; a zero vector is returned unchanged.
#[must_use]
pub fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Build the concept records from the scaffold and prose indexes.
///
/// The embedded text is the class's *header*: title, definition, verbalised
/// typed relations and taxonomy, plus the prose index's full definition when
/// there is one — so a vector query lands on the right IRI rather than on a
/// paragraph that merely mentions it.
#[must_use]
pub fn concept_records(scaffold: &Value, prose: &Value, generation: &str) -> Vec<ConceptRecord> {
    let Some(classes) = scaffold["classes"].as_object() else {
        return Vec::new();
    };
    let prose_pages = prose["pages"].as_object();

    classes
        .iter()
        .map(|(slug, entry)| {
            let key = format!("urn:ngm:class:{slug}");
            let title = entry["t"].as_str().unwrap_or(slug);
            let prose_entry = prose_pages.and_then(|p| p.get(slug));
            let dfull = prose_entry.and_then(|p| p["dfull"].as_str());
            let definition = dfull.or_else(|| entry["d"].as_str()).unwrap_or_default();

            let text = concept_text(title, definition, entry);
            ConceptRecord {
                id: format!("loom:ontology-corpus:{key}"),
                key,
                text,
                metadata: json!({
                    "slug": slug,
                    "title": title,
                    "domain": entry["dom"].as_str().unwrap_or_default(),
                    "maturity": entry["m"].as_str().unwrap_or_default(),
                    "quality": entry["q"].clone(),
                    "has_prose": prose_entry.is_some(),
                    "generation": generation,
                }),
                embedding: None,
            }
        })
        .collect()
}

/// The embedded header text for one class: title, definition, domain,
/// taxonomy and verbalised relations.
fn concept_text(title: &str, definition: &str, entry: &Value) -> String {
    let mut text = format!("{title}\n{definition}");
    if let Some(dom) = entry["dom"].as_str().filter(|d| !d.is_empty()) {
        let _ = write!(text, "\nDomain: {dom}");
    }
    if let Some(sup) = entry["sup"].as_array().filter(|a| !a.is_empty()) {
        let _ = write!(text, "\nIs a: {}", join_slugs(sup));
    }
    if let Some(rel) = entry["rel"].as_object() {
        for (predicate, targets) in rel {
            if let Some(list) = targets.as_array().filter(|a| !a.is_empty()) {
                let _ = write!(text, "\n{predicate}: {}", join_slugs(list));
            }
        }
    }
    text
}

fn join_slugs(items: &[Value]) -> String {
    items
        .iter()
        .filter_map(Value::as_str)
        .map(|s| s.replace('-', " "))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Embed every record in batches, in place.
///
/// # Errors
/// Propagates the embedder's failure. A partial batch is never written: the
/// caller sees an error rather than a half-embedded bundle.
pub fn embed_all(
    records: &mut [ConceptRecord],
    embedder: &dyn Embedder,
    batch: usize,
) -> anyhow::Result<()> {
    let batch = batch.max(1);
    for chunk in records.chunks_mut(batch) {
        let texts: Vec<String> = chunk.iter().map(|r| r.text.clone()).collect();
        let vectors = embedder.embed(&texts)?;
        for (record, vector) in chunk.iter_mut().zip(vectors) {
            record.embedding = Some(vector);
        }
    }
    Ok(())
}

/// Serialise the records as JSONL — the `.rvdb` payload.
///
/// # Errors
/// Propagates a `serde_json` failure.
pub fn to_jsonl(records: &[ConceptRecord]) -> serde_json::Result<String> {
    let mut out = String::new();
    for r in records {
        out.push_str(&serde_json::to_string(r)?);
        out.push('\n');
    }
    Ok(out)
}

/// The `.generation.json` sidecar Loom's ingest law requires.
#[must_use]
pub fn sidecar(generation: &str, records: usize, content_digest: &str) -> Value {
    json!({
        "generation": generation,
        "embedding_model": MODEL_ID,
        "dimensions": DIMENSIONS,
        "records": records,
        "content_digest": content_digest,
        "index": {
            "am": "hnsw",
            "m": 16,
            "ef_construction": 128,
            "build": "serial",
            "note": "Rebuild non-concurrently after a bulk load. \
                     CREATE INDEX CONCURRENTLY double-inserts on the ruvector HNSW \
                     access method and is never correct here.",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(usize);

    impl Embedder for Fake {
        fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let mut v = vec![0.0f32; self.0];
                    #[allow(clippy::cast_precision_loss)]
                    {
                        v[0] = (i + 1) as f32;
                    }
                    v
                })
                .collect())
        }
    }

    fn scaffold() -> Value {
        json!({
            "version": 1,
            "classes": {
                "knowledge-graph": {
                    "t": "Knowledge Graph",
                    "d": "A graph of entities.",
                    "dom": "spatial-computing",
                    "q": 0.35,
                    "m": "established",
                    "sup": ["content-and-assets"],
                    "isup": [],
                    "rel": { "requires": ["ontology", "triple-store"] },
                    "bl": []
                }
            }
        })
    }

    #[test]
    fn a_record_carries_the_iri_key_and_the_idempotent_id() {
        let r = &concept_records(&scaffold(), &json!({}), "visionGraph@abc")[0];
        assert_eq!(r.key, "urn:ngm:class:knowledge-graph");
        assert_eq!(r.id, "loom:ontology-corpus:urn:ngm:class:knowledge-graph");
        assert_eq!(r.metadata["generation"], "visionGraph@abc");
        assert_eq!(r.metadata["has_prose"], false);
    }

    #[test]
    fn the_embedded_text_verbalises_the_structure() {
        let r = &concept_records(&scaffold(), &json!({}), "g")[0];
        assert!(r.text.starts_with("Knowledge Graph\nA graph of entities."));
        assert!(r.text.contains("Domain: spatial-computing"));
        assert!(r.text.contains("Is a: content and assets"));
        assert!(r.text.contains("requires: ontology, triple store"));
    }

    #[test]
    fn the_prose_full_definition_wins_over_the_truncated_one() {
        let prose = json!({ "pages": { "knowledge-graph": { "dfull": "The long form." } } });
        let r = &concept_records(&scaffold(), &prose, "g")[0];
        assert!(r.text.contains("The long form."));
        assert!(!r.text.contains("A graph of entities."));
        assert_eq!(r.metadata["has_prose"], true);
    }

    #[test]
    fn embedding_fills_every_record_and_normalises() {
        let mut records = concept_records(&scaffold(), &json!({}), "g");
        embed_all(&mut records, &Fake(DIMENSIONS), 96).unwrap();
        let v = records[0].embedding.as_ref().unwrap();
        assert_eq!(v.len(), DIMENSIONS);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn normalise_leaves_a_zero_vector_alone() {
        assert_eq!(normalise(vec![0.0; 4]), vec![0.0; 4]);
    }

    #[test]
    fn the_sidecar_states_the_locked_model_and_the_index_law() {
        let s = sidecar("visionGraph@abc", 8146, "deadbeef");
        assert_eq!(s["embedding_model"], MODEL_ID);
        assert_eq!(s["dimensions"], 384);
        assert_eq!(s["records"], 8146);
        assert_eq!(s["index"]["build"], "serial");
        assert!(s["index"]["note"]
            .as_str()
            .unwrap()
            .contains("CONCURRENTLY"));
    }

    #[test]
    fn jsonl_is_one_record_per_line() {
        let records = concept_records(&scaffold(), &json!({}), "g");
        let jsonl = to_jsonl(&records).unwrap();
        assert_eq!(jsonl.lines().count(), 1);
        assert!(jsonl.ends_with('\n'));
    }

    #[test]
    fn an_empty_scaffold_yields_no_records() {
        assert!(concept_records(&json!({}), &json!({}), "g").is_empty());
    }
}
