//! pocket-tts client: `POST /v1/audio/speech` with `response_format:"pcm", stream:true`
//! returns chunked raw PCM, s16le mono at 24 kHz (`x-sample-rate: 24000`,
//! `x-audio-format: s16le`, verified against the running service).

use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::egress::{EgressAllowlist, EgressError};

/// Output sample rate.
pub const SAMPLE_RATE: u32 = 24_000;
/// Longest text sent to TTS in one request (characters).
pub const MAX_INPUT_CHARS: usize = 1_000;

/// TTS failures.
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    /// Not on the allowlist.
    #[error(transparent)]
    Egress(#[from] EgressError),
    /// Transport failure.
    #[error("tts request: {0}")]
    Http(String),
    /// Non-2xx response.
    #[error("tts status {0}")]
    Status(u16),
}

/// The request body for `text`, truncated at a character boundary.
pub fn speech_request(text: &str, voice: &str) -> Value {
    let input: String = text.chars().take(MAX_INPUT_CHARS).collect();
    json!({ "input": input, "voice": voice, "response_format": "pcm", "stream": true })
}

/// Keeps chunk boundaries on whole 16-bit samples when the HTTP body splits mid-sample.
#[derive(Debug, Clone, Default)]
pub struct S16Aligner {
    carry: Option<u8>,
}

impl S16Aligner {
    /// Feed one body chunk; returns an even-length slice of whole samples.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(chunk.len() + 1);
        out.extend(self.carry.take());
        out.extend_from_slice(chunk);
        if out.len() % 2 == 1 {
            self.carry = out.pop();
        }
        out
    }
}

/// pocket-tts client.
#[derive(Clone)]
pub struct TtsClient {
    http: reqwest::Client,
    base: String,
    voice: String,
    allow: EgressAllowlist,
}

impl TtsClient {
    /// A client for `base` (e.g. `http://pocket-tts:8000`).
    pub fn new(allow: &EgressAllowlist, base: &str, voice: &str) -> Self {
        Self {
            http: allow.http_client(Duration::from_secs(60)),
            base: base.trim_end_matches('/').to_string(),
            voice: voice.to_string(),
            allow: allow.clone(),
        }
    }

    /// Stream speech for `text` as whole-sample s16le chunks.
    pub async fn speak(
        &self,
        text: &str,
    ) -> Result<impl Stream<Item = Result<Bytes, TtsError>> + use<>, TtsError> {
        let url = format!("{}/v1/audio/speech", self.base);
        self.allow.check(&url)?;
        let resp = self
            .http
            .post(&url)
            .json(&speech_request(text, &self.voice))
            .send()
            .await
            .map_err(|e| TtsError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(TtsError::Status(resp.status().as_u16()));
        }
        let mut aligner = S16Aligner::default();
        Ok(resp.bytes_stream().map(move |c| match c {
            Ok(b) => Ok(Bytes::from(aligner.push(&b))),
            Err(e) => Err(TtsError::Http(e.to_string())),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape_matches_pocket_tts() {
        let r = speech_request("hello", "alba");
        assert_eq!(
            r,
            json!({"input":"hello","voice":"alba","response_format":"pcm","stream":true})
        );
        let long = "é".repeat(MAX_INPUT_CHARS + 5);
        assert_eq!(
            speech_request(&long, "alba")["input"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            MAX_INPUT_CHARS
        );
    }

    #[test]
    fn aligner_keeps_whole_samples() {
        let mut a = S16Aligner::default();
        assert_eq!(a.push(&[1, 2, 3]), vec![1, 2]);
        assert_eq!(a.push(&[4]), vec![3, 4]);
        assert_eq!(a.push(&[5, 6]), vec![5, 6]);
        assert_eq!(a.push(&[]), Vec::<u8>::new());
    }

    #[tokio::test]
    async fn unlisted_tts_host_is_refused_before_any_io() {
        let allow = EgressAllowlist::from_urls(["http://pocket-tts:8000"]).unwrap();
        let c = TtsClient::new(&allow, "http://elsewhere:8000", "alba");
        assert!(matches!(c.speak("x").await, Err(TtsError::Egress(_))));
    }
}
