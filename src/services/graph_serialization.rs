use crate::models::graph::GraphData;
use crate::models::graph_export::*;
use crate::time;
use anyhow::{anyhow, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use xml::writer::{EmitterConfig, XmlEvent};

pub struct GraphSerializationService {
    pub storage_path: PathBuf,
    pub max_file_size: u64,
    pub compression_level: u32,
}

impl GraphSerializationService {
    
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            storage_path,
            max_file_size: 100 * 1024 * 1024, 
            compression_level: 6,             
        }
    }

    
    pub async fn export_graph(
        &self,
        graph: &GraphData,
        request: &ExportRequest,
    ) -> Result<ExportResponse> {
        let export_id = Uuid::new_v4().to_string();
        let filename = format!(
            "{}_{}.{}",
            export_id,
            time::timestamp_seconds(),
            request.format
        );
        let file_path = self.storage_path.join("exports").join(&filename);

        
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        
        let serialized_data = match request.format {
            ExportFormat::Json => self.serialize_to_json(graph, request)?,
            ExportFormat::Gexf => self.serialize_to_gexf(graph, request)?,
            ExportFormat::Graphml => self.serialize_to_graphml(graph, request)?,
            ExportFormat::Csv => self.serialize_to_csv(graph, request)?,
            ExportFormat::Dot => self.serialize_to_dot(graph, request)?,
        };

        
        let (final_data, compressed, file_size) = if request.compress {
            let compressed_data = self.compress_data(&serialized_data)?;
            let size = compressed_data.len() as u64;
            (compressed_data, true, size)
        } else {
            let size = serialized_data.len() as u64;
            (serialized_data.into_bytes(), false, size)
        };

        
        if file_size > self.max_file_size {
            return Err(anyhow!(
                "Export file size exceeds limit: {} bytes",
                file_size
            ));
        }

        
        fs::write(&file_path, &final_data)?;

        let download_url = format!("/api/graph/download/{}", export_id);
        let expires_at = time::now() + chrono::Duration::hours(24);

        Ok(ExportResponse {
            export_id,
            format: request.format.clone(),
            file_size,
            compressed,
            download_url,
            expires_at,
        })
    }

    
    pub async fn create_shared_graph(
        &self,
        graph: &GraphData,
        request: &ShareRequest,
    ) -> Result<(SharedGraph, ShareResponse)> {
        
        let export_request = ExportRequest {
            format: request.export_format.clone(),
            graph_id: request.graph_id.clone(),
            include_metadata: request.include_metadata,
            compress: true, 
            custom_attributes: None,
        };

        let export_response = self.export_graph(graph, &export_request).await?;

        
        let share_id = Uuid::new_v4();
        let shared_filename = format!("{}.{}", share_id, request.export_format);
        let shared_path = self.storage_path.join("shared").join(&shared_filename);

        
        if let Some(parent) = shared_path.parent() {
            fs::create_dir_all(parent)?;
        }

        
        let export_path = self.storage_path.join("exports").join(format!(
            "{}_{}.{}",
            export_response.export_id,
            export_response.expires_at.timestamp(),
            export_response.format
        ));
        fs::rename(export_path, &shared_path)?;

        
        let mut shared_graph = SharedGraph::new(
            request.title.clone(),
            request.description.clone(),
            None, 
            shared_path.to_string_lossy().to_string(),
            export_response.file_size,
            true, 
            request.export_format.clone(),
            graph.nodes.len() as u32,
            graph.edges.len() as u32,
        );

        shared_graph.id = share_id;
        shared_graph.is_public = request.is_public;

        
        if let Some(hours) = request.expires_in_hours {
            shared_graph.set_expiration(hours);
        }

        
        shared_graph.max_access_count = request.max_access_count;

        
        if let Some(password) = &request.password {
            shared_graph.password_hash = Some(bcrypt::hash(password, bcrypt::DEFAULT_COST)?);
        }

        let share_url = format!("/api/graph/shared/{}", share_id);
        let share_response = ShareResponse {
            share_id,
            share_url,
            qr_code_url: None, 
            expires_at: shared_graph.expires_at,
            created_at: shared_graph.created_at,
        };

        Ok((shared_graph, share_response))
    }

    
    fn compress_data(&self, data: &str) -> Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::new(self.compression_level));
        encoder.write_all(data.as_bytes())?;
        Ok(encoder.finish()?)
    }

    
    fn serialize_to_json(&self, graph: &GraphData, request: &ExportRequest) -> Result<String> {
        let mut export_data = serde_json::Map::new();

        
        export_data.insert("nodes".to_string(), serde_json::to_value(&graph.nodes)?);
        export_data.insert("edges".to_string(), serde_json::to_value(&graph.edges)?);

        
        if request.include_metadata {
            let mut metadata = serde_json::Map::new();
            metadata.insert(
                "node_count".to_string(),
                serde_json::Value::Number(serde_json::Number::from(graph.nodes.len())),
            );
            metadata.insert(
                "edge_count".to_string(),
                serde_json::Value::Number(serde_json::Number::from(graph.edges.len())),
            );
            metadata.insert(
                "exported_at".to_string(),
                serde_json::Value::String(time::format_iso8601(&time::now())),
            );
            export_data.insert("metadata".to_string(), serde_json::Value::Object(metadata));
        }

        Ok(crate::utils::json::to_json_pretty(&export_data)?)
    }

    
    fn serialize_to_gexf(&self, graph: &GraphData, request: &ExportRequest) -> Result<String> {
        let mut buffer = Vec::new();
        {
            let mut writer = EmitterConfig::new()
                .perform_indent(true)
                .create_writer(&mut buffer);

            
            writer.write(
                XmlEvent::start_element("gexf")
                    .attr("xmlns", "http://www.gexf.net/1.2draft")
                    .attr("version", "1.2"),
            )?;

            
            writer.write(
                XmlEvent::start_element("graph")
                    .attr("mode", "static")
                    .attr("defaultedgetype", "undirected"),
            )?;

            
            writer.write(XmlEvent::start_element("nodes"))?;
            for node in &graph.nodes {
                writer.write(
                    XmlEvent::start_element("node")
                        .attr("id", &node.id.to_string())
                        .attr("label", &node.label),
                )?;
                writer.write(XmlEvent::end_element())?; 
            }
            writer.write(XmlEvent::end_element())?; 

            
            writer.write(XmlEvent::start_element("edges"))?;
            for (idx, edge) in graph.edges.iter().enumerate() {
                writer.write(
                    XmlEvent::start_element("edge")
                        .attr("id", &idx.to_string())
                        .attr("source", &edge.source.to_string())
                        .attr("target", &edge.target.to_string())
                        .attr("weight", &edge.weight.to_string()),
                )?;
                writer.write(XmlEvent::end_element())?; 
            }
            writer.write(XmlEvent::end_element())?; 

            writer.write(XmlEvent::end_element())?; 
            writer.write(XmlEvent::end_element())?; 
        }

        Ok(String::from_utf8(buffer)?)
    }

    
    fn serialize_to_graphml(&self, graph: &GraphData, request: &ExportRequest) -> Result<String> {
        let mut buffer = Vec::new();
        {
            let mut writer = EmitterConfig::new()
                .perform_indent(true)
                .create_writer(&mut buffer);

            
            writer.write(XmlEvent::start_element("graphml")
                .attr("xmlns", "http://graphml.graphdrawing.org/xmlns")
                .attr("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance")
                .attr("xsi:schemaLocation", "http://graphml.graphdrawing.org/xmlns http://graphml.graphdrawing.org/xmlns/1.0/graphml.xsd"))?;

            
            writer.write(
                XmlEvent::start_element("key")
                    .attr("id", "weight")
                    .attr("for", "edge")
                    .attr("attr.name", "weight")
                    .attr("attr.type", "double"),
            )?;
            writer.write(XmlEvent::end_element())?;

            
            writer.write(
                XmlEvent::start_element("graph")
                    .attr("id", "G")
                    .attr("edgedefault", "undirected"),
            )?;

            
            for node in &graph.nodes {
                writer.write(XmlEvent::start_element("node").attr("id", &node.id.to_string()))?;
                writer.write(XmlEvent::end_element())?;
            }

            
            for edge in &graph.edges {
                writer.write(
                    XmlEvent::start_element("edge")
                        .attr("source", &edge.source.to_string())
                        .attr("target", &edge.target.to_string()),
                )?;

                let weight = edge.weight;
                writer.write(XmlEvent::start_element("data").attr("key", "weight"))?;
                writer.write(XmlEvent::characters(&weight.to_string()))?;
                writer.write(XmlEvent::end_element())?;

                writer.write(XmlEvent::end_element())?;
            }

            writer.write(XmlEvent::end_element())?; 
            writer.write(XmlEvent::end_element())?; 
        }

        Ok(String::from_utf8(buffer)?)
    }

    
    fn serialize_to_csv(&self, graph: &GraphData, _request: &ExportRequest) -> Result<String> {
        let mut csv_data = String::from("source,target,weight\n");

        for edge in &graph.edges {
            csv_data.push_str(&format!(
                "{},{},{}\n",
                edge.source, edge.target, edge.weight
            ));
        }

        Ok(csv_data)
    }

    
    fn serialize_to_dot(&self, graph: &GraphData, _request: &ExportRequest) -> Result<String> {
        let mut dot_data = String::from("graph G {\n");

        
        for node in &graph.nodes {
            let label = &node.label;
            dot_data.push_str(&format!("  {} [label=\"{}\"];\n", node.id, label));
        }

        
        for edge in &graph.edges {
            let weight = edge.weight;
            dot_data.push_str(&format!(
                "  {} -- {} [weight={}];\n",
                edge.source, edge.target, weight
            ));
        }

        dot_data.push_str("}\n");
        Ok(dot_data)
    }

    
    pub async fn cleanup_expired_files(&self) -> Result<u64> {
        let mut cleaned_count = 0;

        
        let exports_dir = self.storage_path.join("exports");
        if exports_dir.exists() {
            cleaned_count += self.cleanup_directory(&exports_dir, 24 * 60 * 60).await?;
            
        }

        Ok(cleaned_count)
    }

    
    async fn cleanup_directory(&self, dir: &Path, max_age_seconds: u64) -> Result<u64> {
        let mut count = 0;
        let cutoff_time =
            std::time::SystemTime::now() - std::time::Duration::from_secs(max_age_seconds);

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(created) = metadata.created() {
                            if created < cutoff_time {
                                if fs::remove_file(entry.path()).is_ok() {
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
use crate::utils::json::{from_json, to_json};

    #[tokio::test]
    async fn test_json_serialization() {
        let temp_dir = tempdir().unwrap();
        let service = GraphSerializationService::new(temp_dir.path().to_path_buf());

        let mut graph = GraphData::new();
        let mut node = crate::models::node::Node::new("node_1".to_string())
            .with_label("Node 1".to_string())
            .with_position(0.0, 0.0, 0.0);
        node.id = 1;
        graph.nodes.push(node);

        let request = ExportRequest {
            format: ExportFormat::Json,
            ..Default::default()
        };

        let result = service.export_graph(&graph, &request).await;
        assert!(result.is_ok());
    }
}
