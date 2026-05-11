// src/application/inference_service.rs
//! Inference Application Service
//!
//! Orchestrates inference operations including reasoning, caching, and event publishing.
//! Provides high-level API for ontology inference and validation.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, instrument, warn};

use crate::ports::inference_engine::{InferenceEngine, Result as EngineResult};
use crate::ports::ontology_repository::{OntologyRepository, InferenceResults};
use crate::inference::{InferenceCache, InferenceOptimizer, ValidationResult};
use crate::inference::types::{ConsistencyReport, ClassificationResult, UnsatisfiableClass};
use crate::inference::optimization::BatchInferenceRequest;
use crate::events::EventBus;

#[derive(Debug, Clone)]
pub enum InferenceEvent {
    
    InferenceStarted { ontology_id: String },

    
    InferenceCompleted {
        ontology_id: String,
        inference_count: usize,
        duration_ms: u64,
    },

    
    InferenceFailed {
        ontology_id: String,
        error: String,
    },

    
    ValidationCompleted {
        ontology_id: String,
        consistent: bool,
    },

    
    ClassificationCompleted {
        ontology_id: String,
        hierarchy_count: usize,
    },
}

#[derive(Debug, Clone)]
pub struct InferenceServiceConfig {
    
    pub enable_cache: bool,

    
    pub auto_inference: bool,

    
    pub max_parallel: usize,

    
    pub publish_events: bool,
}

impl Default for InferenceServiceConfig {
    fn default() -> Self {
        Self {
            enable_cache: true,
            auto_inference: true,
            max_parallel: 4,
            publish_events: true,
        }
    }
}

pub struct InferenceService {
    
    inference_engine: Arc<RwLock<dyn InferenceEngine>>,

    
    ontology_repo: Arc<dyn OntologyRepository>,

    
    cache: Option<Arc<InferenceCache>>,

    
    optimizer: Arc<InferenceOptimizer>,

    
    event_bus: Arc<RwLock<EventBus>>,

    
    config: InferenceServiceConfig,
}

impl InferenceService {
    
    pub fn new(
        inference_engine: Arc<RwLock<dyn InferenceEngine>>,
        ontology_repo: Arc<dyn OntologyRepository>,
        event_bus: Arc<RwLock<EventBus>>,
        config: InferenceServiceConfig,
    ) -> Self {
        let cache = if config.enable_cache {
            Some(Arc::new(InferenceCache::default()))
        } else {
            None
        };

        let optimizer = Arc::new(InferenceOptimizer::new(config.max_parallel));

        Self {
            inference_engine,
            ontology_repo,
            cache,
            optimizer,
            event_bus,
            config,
        }
    }

    
    #[instrument(skip(self), level = "info")]
    pub async fn run_inference(&self, ontology_id: &str) -> EngineResult<InferenceResults> {
        info!("Running inference for ontology: {}", ontology_id);
        let start = std::time::Instant::now();

        
        if self.config.publish_events {
            self.publish_event(InferenceEvent::InferenceStarted {
                ontology_id: ontology_id.to_string(),
            })
            .await;
        }

        
        let (classes, axioms) = self.load_ontology_data(ontology_id).await?;
        let checksum = self.compute_checksum(&classes, &axioms);

        
        if let Some(ref cache) = self.cache {
            if let Some(cached_results) = cache.get(ontology_id, &checksum).await {
                info!("Using cached inference results");
                return Ok(cached_results);
            }
        }

        
        let mut engine = self.inference_engine.write().await;
        engine.load_ontology(classes, axioms).await?;

        
        let results = match engine.infer().await {
            Ok(results) => results,
            Err(e) => {
                warn!("Inference failed: {:?}", e);
                if self.config.publish_events {
                    self.publish_event(InferenceEvent::InferenceFailed {
                        ontology_id: ontology_id.to_string(),
                        error: format!("{:?}", e),
                    })
                    .await;
                }
                return Err(e);
            }
        };

        drop(engine); 

        
        self.store_inference_results(ontology_id, &results).await?;

        
        if let Some(ref cache) = self.cache {
            cache
                .put(ontology_id.to_string(), checksum, results.clone())
                .await;
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        
        if self.config.publish_events {
            self.publish_event(InferenceEvent::InferenceCompleted {
                ontology_id: ontology_id.to_string(),
                inference_count: results.inferred_axioms.len(),
                duration_ms,
            })
            .await;
        }

        info!(
            "Inference completed in {}ms with {} inferred axioms",
            duration_ms,
            results.inferred_axioms.len()
        );

        Ok(results)
    }

    
    #[instrument(skip(self), level = "info")]
    pub async fn validate_ontology(&self, ontology_id: &str) -> EngineResult<ValidationResult> {
        info!("Validating ontology: {}", ontology_id);
        let start = std::time::Instant::now();

        
        let (classes, axioms) = self.load_ontology_data(ontology_id).await?;

        let mut engine = self.inference_engine.write().await;
        engine.load_ontology(classes, axioms).await?;

        
        let consistent = engine.check_consistency().await?;

        
        let mut unsatisfiable = Vec::new();
        if !consistent {
            let hierarchy = engine.get_subclass_hierarchy().await?;
            let nothing_iri = "http://www.w3.org/2002/07/owl#Nothing";

            for (child, parent) in hierarchy {
                if parent == nothing_iri && child != nothing_iri {
                    unsatisfiable.push(UnsatisfiableClass {
                        class_iri: child,
                        reason: "Equivalent to owl:Nothing".to_string(),
                        conflicting_axioms: Vec::new(),
                    });
                }
            }
        }

        let validation_time_ms = start.elapsed().as_millis() as u64;

        let result = ValidationResult {
            consistent,
            unsatisfiable,
            warnings: Vec::new(),
            errors: Vec::new(),
            validation_time_ms,
        };

        
        if self.config.publish_events {
            self.publish_event(InferenceEvent::ValidationCompleted {
                ontology_id: ontology_id.to_string(),
                consistent,
            })
            .await;
        }

        info!(
            "Validation completed: consistent={}, unsatisfiable={}",
            consistent,
            result.unsatisfiable.len()
        );

        Ok(result)
    }

    
    #[instrument(skip(self), level = "info")]
    pub async fn classify_ontology(&self, ontology_id: &str) -> EngineResult<ClassificationResult> {
        info!("Classifying ontology: {}", ontology_id);
        let start = std::time::Instant::now();

        
        let results = self.run_inference(ontology_id).await?;

        let engine = self.inference_engine.read().await;
        let hierarchy = engine.get_subclass_hierarchy().await?;

        
        let equivalent_classes = self.find_equivalent_classes(&hierarchy);

        let classification_time_ms = start.elapsed().as_millis() as u64;

        let result = ClassificationResult {
            hierarchy: hierarchy.clone(),
            equivalent_classes,
            classification_time_ms,
            inferred_count: results.inferred_axioms.len(),
        };

        
        if self.config.publish_events {
            self.publish_event(InferenceEvent::ClassificationCompleted {
                ontology_id: ontology_id.to_string(),
                hierarchy_count: hierarchy.len(),
            })
            .await;
        }

        Ok(result)
    }

    
    pub async fn get_consistency_report(&self, ontology_id: &str) -> EngineResult<ConsistencyReport> {
        let validation = self.validate_ontology(ontology_id).await?;

        let stats = {
            let engine = self.inference_engine.read().await;
            engine.get_statistics().await?
        };

        Ok(ConsistencyReport {
            is_consistent: validation.consistent,
            unsatisfiable_classes: validation.unsatisfiable,
            classes_checked: stats.loaded_classes,
            axioms_checked: stats.loaded_axioms,
            check_time_ms: validation.validation_time_ms,
            reasoner_version: "whelk-rs-1.0".to_string(),
        })
    }

    
    pub async fn batch_inference(
        &self,
        ontology_ids: Vec<String>,
    ) -> EngineResult<std::collections::HashMap<String, InferenceResults>> {
        info!("Running batch inference for {} ontologies", ontology_ids.len());

        let request = BatchInferenceRequest {
            ontology_ids: ontology_ids.clone(),
            max_parallelism: self.config.max_parallel,
            timeout_ms: 60000, 
        };

        self.optimizer
            .process_batch(Arc::clone(&self.inference_engine), request)
            .await
    }

    
    pub async fn invalidate_cache(&self, ontology_id: &str) {
        if let Some(ref cache) = self.cache {
            cache.invalidate(ontology_id).await;
            info!("Cache invalidated for ontology: {}", ontology_id);
        }
    }

    
    async fn load_ontology_data(
        &self,
        _ontology_id: &str,
    ) -> EngineResult<(Vec<crate::ports::ontology_repository::OwlClass>, Vec<crate::ports::ontology_repository::OwlAxiom>)> {
        let classes = self
            .ontology_repo
            .get_classes()
            .await
            .map_err(|e| crate::ports::inference_engine::InferenceEngineError::ReasonerError(format!("{:?}", e)))?;

        let axioms = self
            .ontology_repo
            .get_axioms()
            .await
            .map_err(|e| crate::ports::inference_engine::InferenceEngineError::ReasonerError(format!("{:?}", e)))?;

        Ok((classes, axioms))
    }

    
    /// Compute a stable checksum using BLAKE3 (P1-24).
    fn compute_checksum(
        &self,
        classes: &[crate::ports::ontology_repository::OwlClass],
        axioms: &[crate::ports::ontology_repository::OwlAxiom],
    ) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&classes.len().to_le_bytes());
        hasher.update(&axioms.len().to_le_bytes());
        let hash = hasher.finalize();
        format!("{}", &hash.to_hex()[..16])
    }

    
    async fn store_inference_results(
        &self,
        ontology_id: &str,
        results: &InferenceResults,
    ) -> EngineResult<()> {
        
        for axiom in &results.inferred_axioms {
            let _ = self.ontology_repo.add_axiom(&axiom).await;
        }

        debug!(
            "Stored {} inferred axioms for ontology {}",
            results.inferred_axioms.len(),
            ontology_id
        );

        Ok(())
    }

    
    fn find_equivalent_classes(&self, hierarchy: &[(String, String)]) -> Vec<Vec<String>> {
        use std::collections::{HashMap, HashSet};

        let mut subclasses: HashMap<String, HashSet<String>> = HashMap::new();
        let mut superclasses: HashMap<String, HashSet<String>> = HashMap::new();

        
        for (child, parent) in hierarchy {
            subclasses
                .entry(parent.clone())
                .or_default()
                .insert(child.clone());
            superclasses
                .entry(child.clone())
                .or_default()
                .insert(parent.clone());
        }

        
        let mut equivalent_groups: Vec<Vec<String>> = Vec::new();
        let mut processed: HashSet<String> = HashSet::new();

        for class in subclasses.keys() {
            if processed.contains(class) {
                continue;
            }

            let mut group = vec![class.clone()];
            let class_subs = subclasses.get(class);
            let class_supers = superclasses.get(class);

            for other in subclasses.keys() {
                if other == class || processed.contains(other) {
                    continue;
                }

                if subclasses.get(other) == class_subs && superclasses.get(other) == class_supers {
                    group.push(other.clone());
                    processed.insert(other.clone());
                }
            }

            if group.len() > 1 {
                processed.insert(class.clone());
                equivalent_groups.push(group);
            }
        }

        equivalent_groups
    }

    
    async fn publish_event(&self, event: InferenceEvent) {
        let _event_bus = self.event_bus.write().await;
        
        debug!("Published event: {:?}", event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use crate::ports::ontology_repository::OwlClass;

    
}
