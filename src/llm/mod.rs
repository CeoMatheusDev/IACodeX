pub mod openai_compatible;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug)]
pub struct LlmError(pub String);

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for LlmError {}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream_chat(
        &self,
        messages: &[Message],
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<String, LlmError>>, LlmError>;
}
