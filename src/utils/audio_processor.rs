use crate::config::AppFullSettings;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use log::{error, info, warn};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AudioProcessor {
    settings: Arc<RwLock<AppFullSettings>>,
}

impl AudioProcessor {
    pub fn new(settings: Arc<RwLock<AppFullSettings>>) -> Self {
        Self { settings }
    }

    pub async fn process_json_response(
        &self,
        response_data: &[u8],
    ) -> Result<(String, Vec<u8>), String> {
        let _settings = self.settings.read().await;

        let json_response: Value = crate::utils::json::from_json_bytes(response_data)
            .map_err(|e| format!("Failed to parse JSON response: {}", e))?;

        info!(
            "Received JSON response: {}",
            crate::utils::json::to_json_pretty(&json_response)
                .unwrap_or_else(|_| "Unable to prettify JSON".to_string())
        );

        if let Some(error_msg) = json_response["error"].as_str() {
            error!("Error in JSON response: {}", error_msg);
            return Err(format!("Error in JSON response: {}", error_msg));
        }

        let answer = json_response["data"]["answer"]
            .as_str()
            .or_else(|| json_response["answer"].as_str())
            .ok_or_else(|| {
                error!("Text answer not found in JSON response");
                "Text answer not found in JSON response".to_string()
            })?
            .to_string();

        let audio_data = if let Some(audio) = json_response["data"]["audio"].as_str() {
            info!("Found audio data in data.audio");
            BASE64
                .decode(audio)
                .map_err(|e| format!("Failed to decode base64 audio data from data.audio: {}", e))?
        } else if let Some(audio) = json_response["audio"].as_str() {
            info!("Found audio data in root.audio");
            BASE64
                .decode(audio)
                .map_err(|e| format!("Failed to decode base64 audio data from root.audio: {}", e))?
        } else {
            warn!("Audio data not found in JSON response. Available paths:");
            if let Some(obj) = json_response.as_object() {
                for (key, value) in obj {
                    warn!(
                        "- {}: {}",
                        key,
                        match value {
                            Value::Null => "null",
                            Value::Bool(_) => "boolean",
                            Value::Number(_) => "number",
                            Value::String(_) => "string",
                            Value::Array(_) => "array",
                            Value::Object(_) => "object",
                        }
                    );
                }
            }
            return Err("Audio data not found in JSON response".to_string());
        };

        info!(
            "Successfully processed audio data: {} bytes",
            audio_data.len()
        );

        if audio_data.len() >= 44 {
            info!("WAV header: {:?}", &audio_data[..44]);

            if &audio_data[..4] != b"RIFF" || &audio_data[8..12] != b"WAVE" {
                error!("Invalid WAV header detected");
                return Err("Invalid WAV header".to_string());
            }

            let channels = u16::from_le_bytes([audio_data[22], audio_data[23]]);
            let sample_rate = u32::from_le_bytes([
                audio_data[24],
                audio_data[25],
                audio_data[26],
                audio_data[27],
            ]);
            let bits_per_sample = u16::from_le_bytes([audio_data[34], audio_data[35]]);

            info!(
                "WAV format: {} channels, {} Hz, {} bits per sample",
                channels, sample_rate, bits_per_sample
            );
        } else {
            error!(
                "Audio data too short to contain WAV header: {} bytes",
                audio_data.len()
            );
            return Err("Audio data too short".to_string());
        }

        Ok((answer, audio_data))
    }

    pub async fn validate_wav_header(&self, audio_data: &[u8]) -> Result<(), String> {
        if audio_data.len() < 44 {
            return Err("Audio data too short for WAV header".to_string());
        }

        if &audio_data[..4] != b"RIFF" {
            return Err("Missing RIFF header".to_string());
        }

        if &audio_data[8..12] != b"WAVE" {
            return Err("Missing WAVE format".to_string());
        }

        let channels = u16::from_le_bytes([audio_data[22], audio_data[23]]);
        let sample_rate = u32::from_le_bytes([
            audio_data[24],
            audio_data[25],
            audio_data[26],
            audio_data[27],
        ]);
        let bits_per_sample = u16::from_le_bytes([audio_data[34], audio_data[35]]);

        info!(
            "Validated WAV format: {} channels, {} Hz, {} bits per sample",
            channels, sample_rate, bits_per_sample
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use tokio::runtime::Runtime;

    fn create_test_settings() -> Arc<RwLock<AppFullSettings>> {
        let settings = AppFullSettings::default();
        Arc::new(RwLock::new(settings))
    }

    #[test]
    fn test_process_json_response_valid() {
        let rt = Runtime::new().unwrap();
        let settings = create_test_settings();
        let processor = AudioProcessor::new(settings);

        let test_wav = vec![
            b'R', b'I', b'F', b'F', 0x24, 0x00, 0x00, 0x00, b'W', b'A', b'V', b'E', b'f', b'm',
            b't', b' ', 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x44, 0xAC, 0x00, 0x00,
            0x88, 0x58, 0x01, 0x00, 0x02, 0x00, 0x10, 0x00, b'd', b'a', b't', b'a', 0x00, 0x00,
            0x00, 0x00,
        ];

        let json_data = json!({
            "data": {
                "answer": "Test answer",
                "audio": BASE64.encode(test_wav)
            }
        });

        let result = rt.block_on(
            processor.process_json_response(
                crate::utils::json::to_json_bytes(&json_data)
                    .unwrap()
                    .as_slice(),
            ),
        );

        assert!(result.is_ok());
        let (answer, audio) = result.unwrap();
        assert_eq!(answer, "Test answer");
        assert_eq!(&audio[..4], b"RIFF");
    }

    #[test]
    fn test_process_json_response_invalid_wav() {
        let rt = Runtime::new().unwrap();
        let settings = create_test_settings();
        let processor = AudioProcessor::new(settings);

        let invalid_wav = vec![0x00; 44];
        let json_data = json!({
            "data": {
                "answer": "Test answer",
                "audio": BASE64.encode(invalid_wav)
            }
        });

        let result = rt.block_on(
            processor.process_json_response(
                crate::utils::json::to_json_bytes(&json_data)
                    .unwrap()
                    .as_slice(),
            ),
        );

        assert!(result.is_err());
    }
}
