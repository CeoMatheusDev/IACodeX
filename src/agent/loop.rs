use crate::llm::{LlmClient, Message};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, PartialEq)]
pub enum AgentEvent {
    Started,
    TextDelta(String),
    Cancelled,
    Completed,
    Failed(String),
}
pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_iterations: usize,
}
pub struct AgentLoop<C: LlmClient> {
    client: Arc<C>,
    config: AgentConfig,
    history: Vec<Message>,
}
impl<C: LlmClient + 'static> AgentLoop<C> {
    pub fn new(client: C, config: AgentConfig) -> Self {
        Self { client: Arc::new(client), config, history: Vec::new() }
    }
    pub async fn run(
        &mut self,
        user_prompt: String,
        cancel: CancellationToken,
    ) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel(64);
        let mut messages = vec![Message {
            role: "system".into(),
            content: self.config.system_prompt.clone(),
        }];
        messages.extend(self.history.clone());
        messages.push(Message { role: "user".into(), content: user_prompt.clone() });
        self.history.push(Message { role: "user".into(), content: user_prompt });
        // v1: one completion per turn. Tool arguments will arrive in streaming deltas.
        let _ = (&self.config.model, self.config.max_iterations);
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            let _ = tx.send(AgentEvent::Started).await;
            if cancel.is_cancelled() {
                let _ = tx.send(AgentEvent::Cancelled).await;
                return;
            }
            let result = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = tx.send(AgentEvent::Cancelled).await;
                    return;
                }
                result = client.stream_chat(&messages, cancel.clone()) => result,
            };
            let mut stream = match result {
                Ok(stream) => stream,
                Err(error) => {
                    let event = if cancel.is_cancelled() { AgentEvent::Cancelled } else { AgentEvent::Failed(error.to_string()) };
                    let _ = tx.send(event).await;
                    return;
                }
            };
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = tx.send(AgentEvent::Cancelled).await;
                        return;
                    }
                    item = stream.next() => match item {
                        Some(Ok(delta)) => {
                            if !delta.is_empty() && tx.send(AgentEvent::TextDelta(delta)).await.is_err() { return; }
                        }
                        Some(Err(error)) => {
                            let event = if cancel.is_cancelled() { AgentEvent::Cancelled } else { AgentEvent::Failed(error.to_string()) };
                            let _ = tx.send(event).await;
                            return;
                        }
                        None => break,
                    }
                }
            }
            let _ = tx.send(AgentEvent::Completed).await;
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmError;
    use async_trait::async_trait;
    use futures_util::{stream, stream::BoxStream};

    struct MockLlmClient;
    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn stream_chat(
            &self, _: &[Message], _: CancellationToken,
        ) -> Result<BoxStream<'static, Result<String, LlmError>>, LlmError> {
            Ok(Box::pin(stream::iter(vec![Ok("Olá".into()), Ok(" mundo".into())])))
        }
    }
    #[tokio::test]
    async fn streams_events_in_order() {
        let mut agent = AgentLoop::new(MockLlmClient, AgentConfig {
            model: "mock".into(), system_prompt: "test".into(), max_iterations: 1,
        });
        let mut rx = agent.run("oi".into(), CancellationToken::new()).await;
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await { events.push(event); }
        assert_eq!(events, vec![
            AgentEvent::Started,
            AgentEvent::TextDelta("Olá".into()),
            AgentEvent::TextDelta(" mundo".into()),
            AgentEvent::Completed,
        ]);
    }
}
