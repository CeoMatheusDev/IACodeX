use crate::llm::{LlmClient, Message};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;
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
    history: Arc<Mutex<Vec<Message>>>,
}
impl<C: LlmClient + 'static> AgentLoop<C> {
    pub fn new(client: C, config: AgentConfig) -> Self {
        Self { client: Arc::new(client), config, history: Arc::new(Mutex::new(Vec::new())) }
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
        let history = Arc::clone(&self.history);
        messages.extend(history.lock().await.iter().cloned());
        messages.push(Message { role: "user".into(), content: user_prompt.clone() });
        // Commit the turn only after a successful response, keeping history consistent.
        // v1: one completion per turn. Tool arguments will arrive in streaming deltas.
        let _ = (&self.config.model, self.config.max_iterations);
        let client = Arc::clone(&self.client);
        let prompt = user_prompt;
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
            let mut response_text = String::new();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = tx.send(AgentEvent::Cancelled).await;
                        return;
                    }
                    item = stream.next() => match item {
                        Some(Ok(delta)) => {
                            if !delta.is_empty() {
                                response_text.push_str(&delta);
                                if tx.send(AgentEvent::TextDelta(delta)).await.is_err() { return; }
                            }
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
            {
                let mut stored = history.lock().await;
                stored.push(Message { role: "user".into(), content: prompt });
                stored.push(Message { role: "assistant".into(), content: response_text });
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
        let history = agent.history.lock().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].content, "Olá mundo");
    }
}
