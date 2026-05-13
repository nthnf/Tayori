use anyhow::{Context, Result, ensure};
use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::{Client as HttpClient, StatusCode};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Clone, Debug)]
pub struct LlmClient {
    http: HttpClient,
    base_url: String,
    api_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatAnswer {
    pub content: String,
}

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

impl LlmClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::new_with_base_url(api_key, DEFAULT_BASE_URL)
    }

    pub fn new_with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            http: HttpClient::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatAnswer> {
        let response = self
            .http
            .post(self.chat_completions_url())
            .bearer_auth(&self.api_key)
            .json(&ChatCompletionRequest::from_request(request, false))
            .send()
            .await
            .context("failed to send chat completion request")?;

        let status = response.status();
        let body = response.text().await.context("failed to read chat body")?;
        ensure_success(status, &body)?;

        let response = serde_json::from_str::<ChatCompletionResponse>(&body)
            .context("failed to parse chat completion response")?;
        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .unwrap_or_default();

        Ok(ChatAnswer { content })
    }

    pub async fn answer_from_context(
        &self,
        model: &str,
        question: &str,
        context: &str,
    ) -> Result<ChatAnswer> {
        self.chat(answer_from_context_request(model, question, context))
            .await
    }

    pub async fn stream_answer_from_context(
        &self,
        model: &str,
        question: &str,
        context: &str,
    ) -> Result<ChatStream> {
        self.stream_chat(answer_from_context_request(model, question, context))
            .await
    }

    pub async fn summarize_transcript(&self, model: &str, transcript: &str) -> Result<ChatAnswer> {
        self.chat(summarize_transcript_request(model, transcript))
            .await
    }

    pub async fn stream_transcript_summary(
        &self,
        model: &str,
        transcript: &str,
    ) -> Result<ChatStream> {
        self.stream_chat(summarize_transcript_request(model, transcript))
            .await
    }

    pub async fn stream_chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let response = self
            .http
            .post(self.chat_completions_url())
            .bearer_auth(&self.api_key)
            .json(&ChatCompletionRequest::from_request(request, true))
            .send()
            .await
            .context("failed to send streaming chat completion request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI chat stream failed with {status}: {body}");
        }

        let mut bytes = response.bytes_stream();
        let stream = try_stream! {
            let mut buffer = String::new();

            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.context("failed to read streaming chat chunk")?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(event_end) = buffer.find("\n\n") {
                    let raw_event = buffer[..event_end].to_string();
                    buffer.replace_range(..event_end + 2, "");

                    for delta in parse_sse_event_text_deltas(&raw_event)? {
                        yield delta;
                    }
                }
            }

            if !buffer.trim().is_empty() {
                for delta in parse_sse_event_text_deltas(&buffer)? {
                    yield delta;
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

fn answer_from_context_request(model: &str, question: &str, context: &str) -> ChatRequest {
    ChatRequest::new(
        model,
        vec![
            ChatMessage::system(
                "You answer live meeting questions for Tayori. Output only the answer text, with no preamble, labels, citations, or meta-commentary. Use the supplied project context when it is relevant. If the context is missing or insufficient, answer from your general knowledge instead. Be concise and directly answer the detected question.",
            ),
            ChatMessage::user(format!("Project context:\n{context}\n\nDetected question:\n{question}")),
        ],
    )
    .with_temperature(0.2)
}

fn summarize_transcript_request(model: &str, transcript: &str) -> ChatRequest {
    ChatRequest::new(
        model,
        vec![
            ChatMessage::system(
                "Summarize a meeting transcript segment for later retrieval. Capture decisions, requirements, open questions, names, dates, numbers, and action items. Keep it dense and factual.",
            ),
            ChatMessage::user(format!("Transcript segment:\n{transcript}")),
        ],
    )
    .with_temperature(0.1)
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

impl ChatCompletionRequest {
    fn from_request(request: ChatRequest, stream: bool) -> Self {
        Self {
            model: request.model,
            messages: request.messages,
            stream,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamChunk {
    choices: Vec<ChatCompletionStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamChoice {
    delta: ChatCompletionStreamDelta,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamDelta {
    content: Option<String>,
}

fn ensure_success(status: StatusCode, body: &str) -> Result<()> {
    ensure!(
        status.is_success(),
        "OpenAI chat failed with {status}: {body}"
    );
    Ok(())
}

fn parse_sse_event_text_deltas(event: &str) -> Result<Vec<String>> {
    let mut deltas = Vec::new();

    for line in event.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };

        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }

        let chunk = serde_json::from_str::<ChatCompletionStreamChunk>(data)
            .with_context(|| format!("failed to parse chat stream event: {data}"))?;
        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                deltas.push(content);
            }
        }
    }

    Ok(deltas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chat_completion_sse_delta() {
        let event = r#"data: {"choices":[{"delta":{"content":"Hel"}}]}
data: {"choices":[{"delta":{"content":"lo"}}]}
"#;

        let deltas = parse_sse_event_text_deltas(event).unwrap();

        assert_eq!(deltas, ["Hel", "lo"]);
    }

    #[test]
    fn ignores_done_event() {
        let deltas = parse_sse_event_text_deltas("data: [DONE]").unwrap();

        assert!(deltas.is_empty());
    }

    #[test]
    fn serializes_chat_roles_for_openai() {
        let message = ChatMessage::user("hello");
        let json = serde_json::to_value(message).unwrap();

        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hello");
    }

    #[test]
    fn answer_prompt_contains_context_and_question() {
        let request = answer_from_context_request("test-model", "What changed?", "Release notes");

        assert_eq!(request.model, "test-model");
        assert!(request.messages[1].content.contains("Release notes"));
        assert!(request.messages[1].content.contains("What changed?"));
    }

    #[test]
    fn summary_prompt_contains_transcript() {
        let request = summarize_transcript_request("test-model", "Alice assigned Bob the task.");

        assert_eq!(request.model, "test-model");
        assert!(request.messages[1].content.contains("Alice assigned Bob"));
    }
}
