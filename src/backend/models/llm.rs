use crate::backend::search::SearchCandidate;
use anyhow::Result;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;

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

        let mut prompt = String::new();
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
        prompt.push_str(&format!("User's Question: {}\n\nAnswer:", question));

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
