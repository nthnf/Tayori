use crate::backend::search::SearchCandidate;
use anyhow::Result;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LlmResponse {
    pub action_taken: bool,
    pub answer: String,
}

#[derive(Clone)]
pub struct LlmModel {
    pub model_name: String,
    api_key: String,
}

impl LlmModel {
    pub fn new(model_name: String, api_key: String) -> Self {
        Self {
            model_name,
            api_key,
        }
    }

    /// Reusable main prompt instructions demanding a structured JSON response.
    pub fn main_prompt() -> &'static str {
        "You are a meeting assistant. Analyze the user's request/question. \
If it is a valid question, actionable request, or command, respond with a JSON object where \"action_taken\" is true and \"answer\" is your markdown-formatted response. \
If it is noise, chit-chat, incomplete/cut-off speech, or not an actionable question, respond with a JSON object where \"action_taken\" is false and \"answer\" is an empty string (\"\"). \
Do not output any introductory or concluding text outside of the raw JSON object. Your response MUST be valid JSON.\n\n\
Format:\n\
{\n  \"action_taken\": bool,\n  \"answer\": \"string\"\n}"
    }

    /// Appends context and score-specific instructions to the main prompt.
    pub fn build_prompt(
        question: &str,
        context: &[SearchCandidate],
        max_cosine: f64,
        max_bm25: f64,
    ) -> String {
        let mut prompt = String::new();
        prompt.push_str(Self::main_prompt());
        prompt.push_str("\n\n");

        if max_cosine > 0.70 || max_bm25 > 5.0 {
            prompt.push_str("Answer the user's question based ONLY on the following context retrieved from their personal database. Do not use outside information.\n\n");
            for (i, item) in context.iter().enumerate() {
                prompt.push_str(&format!(
                    "[Context {}]\nType: {}\nContent: {}\n\n",
                    i + 1,
                    item.source_type,
                    item.content
                ));
            }
        } else if max_cosine > 0.50 || max_bm25 > 2.0 {
            prompt.push_str("You have been provided some context that might be relevant. Use it if helpful, but you may also rely on your own knowledge.\n\n");
            for (i, item) in context.iter().enumerate() {
                prompt.push_str(&format!(
                    "[Context {}]\nType: {}\nContent: {}\n\n",
                    i + 1,
                    item.source_type,
                    item.content
                ));
            }
        } else {
            prompt.push_str("Answer the user's question directly from your own knowledge.\n\n");
        }

        prompt.push_str(&format!("User's Question: {}\n\nJSON Response:", question));
        prompt
    }

    pub async fn ask_stream(
        &self,
        question: &str,
        context: Vec<SearchCandidate>,
        max_cosine: f64,
        max_bm25: f64,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
        use futures::StreamExt;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::OpenAI)
            .api_key(self.api_key.clone())
            .model(self.model_name.clone())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build LLM: {}", e))?;

        let prompt = Self::build_prompt(question, &context, max_cosine, max_bm25);
        let messages = vec![ChatMessage::user().content(prompt).build()];

        let stream = llm
            .chat_stream(&messages)
            .await
            .map_err(|e| anyhow::anyhow!("Chat stream error: {}", e))?;

        let mapped_stream =
            stream.map(|res| res.map_err(|e| anyhow::anyhow!("Stream error: {}", e)));

        Ok(Box::pin(mapped_stream))
    }
}

pub const KEYRING_SERVICE: &str = "tayori";
pub const KEYRING_USERNAME: &str = "default";

pub fn store_api_key(api_key: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)
        .map_err(|e| anyhow::anyhow!("Failed to create keyring entry: {}", e))?;
    entry
        .set_password(api_key)
        .map_err(|e| anyhow::anyhow!("Failed to store password in keyring: {}", e))?;
    Ok(())
}

pub fn read_api_key() -> Result<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)
        .map_err(|e| anyhow::anyhow!("Failed to create keyring entry: {}", e))?;
    let password = entry
        .get_password()
        .map_err(|e| anyhow::anyhow!("Failed to read password from keyring: {}", e))?;
    Ok(password)
}

pub fn delete_api_key() -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)
        .map_err(|e| anyhow::anyhow!("Failed to create keyring entry: {}", e))?;
    entry
        .delete_credential()
        .map_err(|e| anyhow::anyhow!("Failed to delete password from keyring: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_model_struct_creation() {
        let model = LlmModel::new("gpt-4o".to_string(), "test-api-key".to_string());
        assert_eq!(model.model_name, "gpt-4o");
        assert_eq!(model.api_key, "test-api-key");
    }

    #[test]
    fn test_build_prompt_reusability() {
        let question = "How do I setup pipewire?";
        let context = vec![SearchCandidate {
            id: "doc1".to_string(),
            source_type: "document".to_string(),
            content: "Run apt install pipewire-audio".to_string(),
            raw_score: 0.8,
        }];

        let prompt = LlmModel::build_prompt(question, &context, 0.85, 0.0);
        assert!(prompt.contains(LlmModel::main_prompt()));
        assert!(prompt.contains("Answer the user's question based ONLY on the following context"));
        assert!(prompt.contains("[Context 1]"));
        assert!(prompt.contains("Run apt install pipewire-audio"));
        assert!(prompt.contains("User's Question: How do I setup pipewire?"));
    }

    #[test]
    fn test_llm_response_json_deserialization() {
        let json_str = r#"{"action_taken": true, "answer": "Install pipewire package."}"#;
        let response: LlmResponse = serde_json::from_str(json_str).unwrap();
        assert!(response.action_taken);
        assert_eq!(response.answer, "Install pipewire package.");

        let negative_json = r#"{"action_taken": false, "answer": ""}"#;
        let response_neg: LlmResponse = serde_json::from_str(negative_json).unwrap();
        assert!(!response_neg.action_taken);
        assert_eq!(response_neg.answer, "");
    }

    #[tokio::test]
    async fn test_llm_model_ask_invalid_key() {
        let model = LlmModel::new("gpt-4o".to_string(), "invalid-key".to_string());
        let candidate = SearchCandidate {
            id: "1".to_string(),
            source_type: "document".to_string(),
            content: "some test content".to_string(),
            raw_score: 0.0,
        };
        let res = model
            .ask_stream("What is the content?", vec![candidate], 0.0, 0.0)
            .await;
        assert!(res.is_err());
    }
}
