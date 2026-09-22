// Ultra-Fast Binary Protocol for Settings Updates
// Implements custom binary serialization, delta encoding, and streaming compression

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use flate2::{Compress, Compression, Decompress};
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryMessage {
    GetSetting {
        path_id: u32,
    },
    SetSetting {
        path_id: u32,
        value: BinaryValue,
    },
    BatchGet {
        path_ids: Vec<u32>,
    },
    BatchSet {
        updates: Vec<(u32, BinaryValue)>,
    },
    Delta {
        path_id: u32,
        old_value: BinaryValue,
        new_value: BinaryValue,
    },
    Response {
        success: bool,
        data: Vec<u8>,
    },
    Error {
        code: u16,
        message: String,
    },
    Ping,
    Pong,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryValue {
    Null,
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<BinaryValue>),
    Object(HashMap<String, BinaryValue>),
}

#[derive(Debug, Clone)]
pub struct PathRegistry {
    path_to_id: HashMap<String, u32>,
    id_to_path: HashMap<u32, String>,
    next_id: u32,
}

impl PathRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            next_id: 1,
        };

        let common_paths = vec![
            "visualisation.graphs.knowledge.physics.damping",
            "visualisation.graphs.knowledge.physics.spring_k",
            "visualisation.graphs.knowledge.physics.repel_k",
            "visualisation.graphs.knowledge.physics.max_velocity",
            "visualisation.graphs.knowledge.physics.gravity",
            "visualisation.graphs.knowledge.physics.temperature",
            "visualisation.graphs.knowledge.physics.bounds_size",
            "visualisation.graphs.knowledge.physics.iterations",
            "visualisation.graphs.knowledge.physics.enabled",
        ];

        for path in common_paths {
            registry.register_path(path.to_string());
        }

        registry
    }

    pub fn register_path(&mut self, path: String) -> u32 {
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }

        let id = self.next_id;
        self.next_id += 1;

        self.path_to_id.insert(path.clone(), id);
        self.id_to_path.insert(id, path);

        debug!("Registered path '{}' with ID {}", self.id_to_path[&id], id);
        id
    }

    pub fn get_path_id(&self, path: &str) -> Option<u32> {
        self.path_to_id.get(path).copied()
    }

    pub fn get_path_by_id(&self, id: u32) -> Option<&String> {
        self.id_to_path.get(&id)
    }
}

pub struct BinarySettingsProtocol {
    path_registry: PathRegistry,
    compressor: Compress,
    decompressor: Decompress,
    compression_threshold: usize,
}

impl BinarySettingsProtocol {
    pub fn new() -> Self {
        Self {
            path_registry: PathRegistry::new(),
            compressor: Compress::new(Compression::fast(), false),
            decompressor: Decompress::new(false),
            compression_threshold: 256,
        }
    }

    pub fn json_to_binary_value(&self, value: &Value) -> BinaryValue {
        match value {
            Value::Null => BinaryValue::Null,
            Value::Bool(b) => BinaryValue::Bool(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                        BinaryValue::I32(i as i32)
                    } else {
                        BinaryValue::I64(i)
                    }
                } else if let Some(f) = n.as_f64() {
                    if (f as f32 as f64 - f).abs() < f64::EPSILON * 10.0 {
                        BinaryValue::F32(f as f32)
                    } else {
                        BinaryValue::F64(f)
                    }
                } else {
                    BinaryValue::Null
                }
            }
            Value::String(s) => BinaryValue::String(s.clone()),
            Value::Array(arr) => {
                let binary_arr: Vec<BinaryValue> =
                    arr.iter().map(|v| self.json_to_binary_value(v)).collect();
                BinaryValue::Array(binary_arr)
            }
            Value::Object(obj) => {
                let binary_obj: HashMap<String, BinaryValue> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), self.json_to_binary_value(v)))
                    .collect();
                BinaryValue::Object(binary_obj)
            }
        }
    }

    pub fn binary_value_to_json(&self, value: &BinaryValue) -> Value {
        match value {
            BinaryValue::Null => Value::Null,
            BinaryValue::Bool(b) => Value::Bool(*b),
            BinaryValue::I32(i) => Value::Number((*i).into()),
            BinaryValue::I64(i) => Value::Number((*i).into()),
            BinaryValue::F32(f) => {
                Value::Number(serde_json::Number::from_f64(*f as f64).unwrap_or_default())
            }
            BinaryValue::F64(f) => {
                Value::Number(serde_json::Number::from_f64(*f).unwrap_or_default())
            }
            BinaryValue::String(s) => Value::String(s.clone()),
            BinaryValue::Bytes(b) => Value::String(base64::encode(b)),
            BinaryValue::Array(arr) => {
                let json_arr: Vec<Value> =
                    arr.iter().map(|v| self.binary_value_to_json(v)).collect();
                Value::Array(json_arr)
            }
            BinaryValue::Object(obj) => {
                let json_obj: serde_json::Map<String, Value> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), self.binary_value_to_json(v)))
                    .collect();
                Value::Object(json_obj)
            }
        }
    }

    pub fn serialize_message(&mut self, message: &BinaryMessage) -> Result<Vec<u8>, String> {
        let mut buffer = Vec::new();

        match message {
            BinaryMessage::GetSetting { path_id } => {
                buffer.write_u8(0x01).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(*path_id)
                    .map_err(|e| e.to_string())?;
            }
            BinaryMessage::SetSetting { path_id, value } => {
                buffer.write_u8(0x02).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(*path_id)
                    .map_err(|e| e.to_string())?;
                self.serialize_binary_value(&mut buffer, value)?;
            }
            BinaryMessage::BatchGet { path_ids } => {
                buffer.write_u8(0x03).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(path_ids.len() as u32)
                    .map_err(|e| e.to_string())?;
                for id in path_ids {
                    buffer
                        .write_u32::<LittleEndian>(*id)
                        .map_err(|e| e.to_string())?;
                }
            }
            BinaryMessage::BatchSet { updates } => {
                buffer.write_u8(0x04).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(updates.len() as u32)
                    .map_err(|e| e.to_string())?;
                for (path_id, value) in updates {
                    buffer
                        .write_u32::<LittleEndian>(*path_id)
                        .map_err(|e| e.to_string())?;
                    self.serialize_binary_value(&mut buffer, value)?;
                }
            }
            BinaryMessage::Delta {
                path_id,
                old_value,
                new_value,
            } => {
                buffer.write_u8(0x05).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(*path_id)
                    .map_err(|e| e.to_string())?;

                let delta = self.compute_value_delta(old_value, new_value)?;
                self.serialize_binary_value(&mut buffer, &delta)?;
            }
            BinaryMessage::Response { success, data } => {
                buffer.write_u8(0x06).map_err(|e| e.to_string())?;
                buffer
                    .write_u8(if *success { 1 } else { 0 })
                    .map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(data.len() as u32)
                    .map_err(|e| e.to_string())?;
                buffer.extend_from_slice(data);
            }
            BinaryMessage::Error { code, message } => {
                buffer.write_u8(0x07).map_err(|e| e.to_string())?;
                buffer
                    .write_u16::<LittleEndian>(*code)
                    .map_err(|e| e.to_string())?;
                let msg_bytes = message.as_bytes();
                buffer
                    .write_u32::<LittleEndian>(msg_bytes.len() as u32)
                    .map_err(|e| e.to_string())?;
                buffer.extend_from_slice(msg_bytes);
            }
            BinaryMessage::Ping => {
                buffer.write_u8(0x08).map_err(|e| e.to_string())?;
            }
            BinaryMessage::Pong => {
                buffer.write_u8(0x09).map_err(|e| e.to_string())?;
            }
        }

        if buffer.len() > self.compression_threshold {
            let compressed = self.compress_data(&buffer)?;
            if compressed.len() < buffer.len() {
                let mut final_buffer = vec![0xFF];
                final_buffer.extend(compressed);
                debug!(
                    "Compressed message: {} -> {} bytes ({:.1}% reduction)",
                    buffer.len(),
                    final_buffer.len(),
                    (1.0 - final_buffer.len() as f64 / buffer.len() as f64) * 100.0
                );
                return Ok(final_buffer);
            }
        }

        let mut final_buffer = vec![0x00];
        final_buffer.extend(buffer);
        Ok(final_buffer)
    }

    pub fn deserialize_message(&mut self, data: &[u8]) -> Result<BinaryMessage, String> {
        if data.is_empty() {
            return Err("Empty message data".to_string());
        }

        let mut cursor = Cursor::new(data);
        let compression_flag = cursor.read_u8().map_err(|e| e.to_string())?;

        let payload = if compression_flag == 0xFF {
            let mut compressed = Vec::new();
            cursor
                .read_to_end(&mut compressed)
                .map_err(|e| e.to_string())?;
            self.decompress_data(&compressed)?
        } else {
            let mut uncompressed = Vec::new();
            cursor
                .read_to_end(&mut uncompressed)
                .map_err(|e| e.to_string())?;
            uncompressed
        };

        let mut cursor = Cursor::new(payload);
        let msg_type = cursor.read_u8().map_err(|e| e.to_string())?;

        match msg_type {
            0x01 => {
                let path_id = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                Ok(BinaryMessage::GetSetting { path_id })
            }
            0x02 => {
                let path_id = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                let value = self.deserialize_binary_value(&mut cursor)?;
                Ok(BinaryMessage::SetSetting { path_id, value })
            }
            0x03 => {
                let count = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut path_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    path_ids.push(
                        cursor
                            .read_u32::<LittleEndian>()
                            .map_err(|e| e.to_string())?,
                    );
                }
                Ok(BinaryMessage::BatchGet { path_ids })
            }
            0x04 => {
                let count = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut updates = Vec::with_capacity(count);
                for _ in 0..count {
                    let path_id = cursor
                        .read_u32::<LittleEndian>()
                        .map_err(|e| e.to_string())?;
                    let value = self.deserialize_binary_value(&mut cursor)?;
                    updates.push((path_id, value));
                }
                Ok(BinaryMessage::BatchSet { updates })
            }
            0x05 => {
                let path_id = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                let old_value = self.deserialize_binary_value(&mut cursor)?;
                let new_value = self.deserialize_binary_value(&mut cursor)?;
                Ok(BinaryMessage::Delta {
                    path_id,
                    old_value,
                    new_value,
                })
            }
            0x06 => {
                let success = cursor.read_u8().map_err(|e| e.to_string())? != 0;
                let data_len = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut data = vec![0u8; data_len];
                cursor.read_exact(&mut data).map_err(|e| e.to_string())?;
                Ok(BinaryMessage::Response { success, data })
            }
            0x07 => {
                let code = cursor
                    .read_u16::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                let msg_len = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut msg_bytes = vec![0u8; msg_len];
                cursor
                    .read_exact(&mut msg_bytes)
                    .map_err(|e| e.to_string())?;
                let message = String::from_utf8(msg_bytes).map_err(|e| e.to_string())?;
                Ok(BinaryMessage::Error { code, message })
            }
            0x08 => Ok(BinaryMessage::Ping),
            0x09 => Ok(BinaryMessage::Pong),
            _ => Err(format!("Unknown message type: {}", msg_type)),
        }
    }

    fn serialize_binary_value(
        &self,
        buffer: &mut Vec<u8>,
        value: &BinaryValue,
    ) -> Result<(), String> {
        match value {
            BinaryValue::Null => {
                buffer.write_u8(0x00).map_err(|e| e.to_string())?;
            }
            BinaryValue::Bool(b) => {
                buffer.write_u8(0x01).map_err(|e| e.to_string())?;
                buffer
                    .write_u8(if *b { 1 } else { 0 })
                    .map_err(|e| e.to_string())?;
            }
            BinaryValue::I32(i) => {
                buffer.write_u8(0x02).map_err(|e| e.to_string())?;
                buffer
                    .write_i32::<LittleEndian>(*i)
                    .map_err(|e| e.to_string())?;
            }
            BinaryValue::I64(i) => {
                buffer.write_u8(0x03).map_err(|e| e.to_string())?;
                buffer
                    .write_i64::<LittleEndian>(*i)
                    .map_err(|e| e.to_string())?;
            }
            BinaryValue::F32(f) => {
                buffer.write_u8(0x04).map_err(|e| e.to_string())?;
                buffer
                    .write_f32::<LittleEndian>(*f)
                    .map_err(|e| e.to_string())?;
            }
            BinaryValue::F64(f) => {
                buffer.write_u8(0x05).map_err(|e| e.to_string())?;
                buffer
                    .write_f64::<LittleEndian>(*f)
                    .map_err(|e| e.to_string())?;
            }
            BinaryValue::String(s) => {
                buffer.write_u8(0x06).map_err(|e| e.to_string())?;
                let bytes = s.as_bytes();
                buffer
                    .write_u32::<LittleEndian>(bytes.len() as u32)
                    .map_err(|e| e.to_string())?;
                buffer.extend_from_slice(bytes);
            }
            BinaryValue::Bytes(b) => {
                buffer.write_u8(0x07).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(b.len() as u32)
                    .map_err(|e| e.to_string())?;
                buffer.extend_from_slice(b);
            }
            BinaryValue::Array(arr) => {
                buffer.write_u8(0x08).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(arr.len() as u32)
                    .map_err(|e| e.to_string())?;
                for item in arr {
                    self.serialize_binary_value(buffer, item)?;
                }
            }
            BinaryValue::Object(obj) => {
                buffer.write_u8(0x09).map_err(|e| e.to_string())?;
                buffer
                    .write_u32::<LittleEndian>(obj.len() as u32)
                    .map_err(|e| e.to_string())?;
                for (key, val) in obj {
                    let key_bytes = key.as_bytes();
                    buffer
                        .write_u32::<LittleEndian>(key_bytes.len() as u32)
                        .map_err(|e| e.to_string())?;
                    buffer.extend_from_slice(key_bytes);
                    self.serialize_binary_value(buffer, val)?;
                }
            }
        }
        Ok(())
    }

    fn deserialize_binary_value(
        &self,
        cursor: &mut Cursor<Vec<u8>>,
    ) -> Result<BinaryValue, String> {
        let value_type = cursor.read_u8().map_err(|e| e.to_string())?;

        match value_type {
            0x00 => Ok(BinaryValue::Null),
            0x01 => {
                let b = cursor.read_u8().map_err(|e| e.to_string())? != 0;
                Ok(BinaryValue::Bool(b))
            }
            0x02 => {
                let i = cursor
                    .read_i32::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                Ok(BinaryValue::I32(i))
            }
            0x03 => {
                let i = cursor
                    .read_i64::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                Ok(BinaryValue::I64(i))
            }
            0x04 => {
                let f = cursor
                    .read_f32::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                Ok(BinaryValue::F32(f))
            }
            0x05 => {
                let f = cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| e.to_string())?;
                Ok(BinaryValue::F64(f))
            }
            0x06 => {
                let len = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut bytes = vec![0u8; len];
                cursor.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                let string = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                Ok(BinaryValue::String(string))
            }
            0x07 => {
                let len = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut bytes = vec![0u8; len];
                cursor.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                Ok(BinaryValue::Bytes(bytes))
            }
            0x08 => {
                let len = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(self.deserialize_binary_value(cursor)?);
                }
                Ok(BinaryValue::Array(arr))
            }
            0x09 => {
                let len = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| e.to_string())? as usize;
                let mut obj = HashMap::with_capacity(len);
                for _ in 0..len {
                    let key_len = cursor
                        .read_u32::<LittleEndian>()
                        .map_err(|e| e.to_string())? as usize;
                    let mut key_bytes = vec![0u8; key_len];
                    cursor
                        .read_exact(&mut key_bytes)
                        .map_err(|e| e.to_string())?;
                    let key = String::from_utf8(key_bytes).map_err(|e| e.to_string())?;
                    let value = self.deserialize_binary_value(cursor)?;
                    obj.insert(key, value);
                }
                Ok(BinaryValue::Object(obj))
            }
            _ => Err(format!("Unknown value type: {}", value_type)),
        }
    }

    fn compute_value_delta(
        &self,
        old: &BinaryValue,
        new: &BinaryValue,
    ) -> Result<BinaryValue, String> {
        Ok(new.clone())
    }

    fn compress_data(&mut self, data: &[u8]) -> Result<Vec<u8>, String> {
        let mut compressed = Vec::new();
        let mut output_buffer = vec![0u8; data.len() * 2];

        match self
            .compressor
            .compress_vec(data, &mut output_buffer, flate2::FlushCompress::Finish)
        {
            Ok(flate2::Status::StreamEnd) => {
                let compressed_size = self.compressor.total_out() as usize;
                output_buffer.truncate(compressed_size);
                compressed.extend(output_buffer);
                Ok(compressed)
            }
            _ => Err("Compression failed".to_string()),
        }
    }

    fn decompress_data(&mut self, compressed: &[u8]) -> Result<Vec<u8>, String> {
        let mut decompressed = Vec::new();
        let mut output_buffer = vec![0u8; compressed.len() * 4];

        match self.decompressor.decompress_vec(
            compressed,
            &mut output_buffer,
            flate2::FlushDecompress::Finish,
        ) {
            Ok(flate2::Status::StreamEnd) => {
                let decompressed_size = self.decompressor.total_out() as usize;
                output_buffer.truncate(decompressed_size);
                decompressed.extend(output_buffer);
                Ok(decompressed)
            }
            _ => Err("Decompression failed".to_string()),
        }
    }

    pub fn get_or_register_path(&mut self, path: &str) -> u32 {
        if let Some(id) = self.path_registry.get_path_id(path) {
            return id;
        }
        self.path_registry.register_path(path.to_string())
    }

    pub fn get_path_by_id(&self, id: u32) -> Option<&String> {
        self.path_registry.get_path_by_id(id)
    }

    pub fn calculate_compression_ratio(&self, original_size: usize, compressed_size: usize) -> f64 {
        if original_size == 0 {
            return 0.0;
        }
        1.0 - (compressed_size as f64 / original_size as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_registry() {
        let mut registry = PathRegistry::new();

        let path1 = "test.path.1";
        let path2 = "test.path.2";

        let id1 = registry.register_path(path1.to_string());
        let id2 = registry.register_path(path2.to_string());

        assert_ne!(id1, id2);
        assert_eq!(registry.get_path_id(path1), Some(id1));
        assert_eq!(registry.get_path_id(path2), Some(id2));
        assert_eq!(registry.get_path_by_id(id1), Some(&path1.to_string()));
        assert_eq!(registry.get_path_by_id(id2), Some(&path2.to_string()));
    }

    #[test]
    fn test_binary_value_conversion() {
        let protocol = BinarySettingsProtocol::new();

        let json_value = serde_json::json!({
            "float": 3.14159,
            "integer": 42,
            "boolean": true,
            "string": "test",
            "array": [1, 2, 3],
            "null": null
        });

        let binary_value = protocol.json_to_binary_value(&json_value);
        let converted_back = protocol.binary_value_to_json(&binary_value);

        assert_eq!(converted_back["integer"], json_value["integer"]);
        assert_eq!(converted_back["boolean"], json_value["boolean"]);
        assert_eq!(converted_back["string"], json_value["string"]);
        assert_eq!(converted_back["null"], json_value["null"]);
    }

    #[test]
    fn test_message_serialization() {
        let mut protocol = BinarySettingsProtocol::new();

        let original_msg = BinaryMessage::SetSetting {
            path_id: 1,
            value: BinaryValue::F32(3.14159),
        };

        let serialized = protocol.serialize_message(&original_msg).unwrap();
        let deserialized = protocol.deserialize_message(&serialized).unwrap();

        assert_eq!(original_msg, deserialized);
    }

    #[test]
    fn test_batch_operations() {
        let mut protocol = BinarySettingsProtocol::new();

        let batch_msg = BinaryMessage::BatchSet {
            updates: vec![
                (1, BinaryValue::F32(1.0)),
                (2, BinaryValue::Bool(true)),
                (3, BinaryValue::String("test".to_string())),
            ],
        };

        let serialized = protocol.serialize_message(&batch_msg).unwrap();
        let deserialized = protocol.deserialize_message(&serialized).unwrap();

        assert_eq!(batch_msg, deserialized);
    }
}
