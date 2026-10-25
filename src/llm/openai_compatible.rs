use super::{LlmClient, LlmError, Message};
use async_trait::async_trait;
use futures_util::{stream::BoxStream, StreamExt};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

pub struct OpenAiCompatible {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
}
impl OpenAiCompatible {
    pub fn new(base_url: &str, api_key: String, model: String) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| LlmError(format!("HTTP client: {e}")))?;
        let base = base_url.trim_end_matches('/');
        let endpoint = format!("{base}/chat/completions");
        Ok(Self { http, endpoint, api_key, model })
    }
}
#[async_trait]
impl LlmClient for OpenAiCompatible {
    async fn stream_chat(
        &self,
        messages: &[Message],
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<String, LlmError>>, LlmError> {
        let request = self.http.post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&json!({"model": self.model, "messages": messages, "stream": true}));
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError("Cancelled".into())),
            response = request.send() => response.map_err(|e| LlmError(format!("Connection failed: {e}")))?,
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError(format!("Provider HTTP {status}: {}", body.chars().take(300).collect::<String>())));
        }
        let stream = async_stream::stream! {
            let mut bytes = response.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();
            let mut done = false;
            loop {
                let next = tokio::select! {
                    _ = cancel.cancelled() => break,
                    chunk = bytes.next() => chunk,
                };
                match next {
                    Some(Ok(chunk)) => {
                        buffer.extend_from_slice(&chunk);
                        while let Some(pos) = buffer.iter().position(|byte| *byte == b'\n') {
                            let mut line = buffer.drain(..=pos).collect::<Vec<_>>();
                            line.pop(); // ASCII newline
                            if line.last() == Some(&b'\r') { line.pop(); }
                            let line = match String::from_utf8(line) {
                                Ok(line) => line,
                                Err(e) => { yield Err(LlmError(format!("Invalid SSE UTF-8: {e}"))); return; }
                            };
                            let Some(payload) = line.strip_prefix("data:") else { continue };
                            let payload = payload.trim();
                            if payload == "[DONE]" { done = true; break; }
                            if payload.is_empty() { continue; }
                            match serde_json::from_str::<Value>(payload) {
                                Ok(value) => {
                                    if let Some(content) = value.pointer("/choices/0/delta/content").and_then(Value::as_str) {
                                        if !content.is_empty() { yield Ok(content.to_owned()); }
                                    }
                                }
                                Err(e) => { yield Err(LlmError(format!("Invalid SSE JSON: {e}"))); return; }
                            }
                        }
                        if done { break; }
                    }
                    Some(Err(e)) => { yield Err(LlmError(format!("Streaming disconnected: {e}"))); break; }
                    None => {
                        if !done { yield Err(LlmError("Streaming ended before [DONE]".into())); }
                        break;
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}
