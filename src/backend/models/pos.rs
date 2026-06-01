use anyhow::{Result, anyhow};
use ndarray::Array2;
use ort::{inputs, session::Session, value::Value};
use std::path::Path;
use std::sync::Mutex;
use tokenizers::Tokenizer;

pub struct PosModel {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl PosModel {
    pub fn new(model_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow!("Failed to load tokenizer from file: {}", e))?;

        let session = Session::builder()
            .map_err(|e| anyhow!("Session builder error: {}", e))?
            .with_intra_threads(1)
            .map_err(|e| anyhow!("Failed to set threads: {}", e))?
            .commit_from_file(model_path)
            .map_err(|e| anyhow!("Failed to load model from file: {}", e))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    pub fn extract_entities(&self, text: &str) -> Result<Vec<ExtractedEntity>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow!("Tokenizer error: {}", e))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();

        let seq_len = input_ids.len();
        if seq_len == 0 {
            return Ok(Vec::new());
        }

        let input_ids_arr = Array2::from_shape_vec((1, seq_len), input_ids)?;
        let attention_mask_arr = Array2::from_shape_vec((1, seq_len), attention_mask)?;
        let token_type_ids_arr = Array2::from_shape_vec((1, seq_len), token_type_ids)?;

        let input_ids_tensor = Value::from_array(input_ids_arr)?;
        let attention_mask_tensor = Value::from_array(attention_mask_arr)?;
        let token_type_ids_tensor = Value::from_array(token_type_ids_arr)?;

        // Run Inference
        let mut session_guard = self.session.lock().map_err(|e| anyhow!("Mutex poisoned: {}", e))?;
        let outputs = session_guard
            .run(inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor,
            ])
            .map_err(|e| anyhow!("Inference error: {}", e))?;

        // Extract logits of shape [1, seq_len, 34] for POS model
        let (_, data) = outputs["logits"].try_extract_tensor::<f32>()?;

        // Standard UPOS/PTB labels for MobileBERT POS
        let labels = [
            "O", "CC", "CD", "DT", "EX", "FW", "IN", "JJ", "JJR", "JJS", "MD", "NN", "NNP", "NNPS",
            "NNS", "PDT", "POS", "PRP", "RB", "RBR", "RBS", "RP", "SYM", "TO", "UH", "VB", "VBD",
            "VBG", "VBN", "VBP", "VBZ", "WDT", "WP", "WRB",
        ];

        let mut entities = Vec::new();
        let tokens = encoding.get_tokens();

        let mut current_entity_text = String::new();
        let mut current_entity_type = String::new();

        for (i, token) in tokens.iter().enumerate().take(seq_len) {
            let offset = i * 34;
            if offset + 34 > data.len() {
                break;
            }
            let logits_slice = &data[offset..offset + 34];

            // Argmax
            let mut max_idx = 0;
            let mut max_val = logits_slice[0];
            for (idx, &val) in logits_slice.iter().enumerate() {
                if val > max_val {
                    max_val = val;
                    max_idx = idx;
                }
            }

            let label = labels[max_idx];

            // Ignore special tokens like [CLS], [SEP], [PAD]
            if token == "[CLS]" || token == "[SEP]" || token == "[PAD]" {
                continue;
            }

            // Group Nouns (NN*) and Adjectives (JJ*) into Noun Phrases
            if label.starts_with("NN") || label.starts_with("JJ") {
                if current_entity_text.is_empty() {
                    current_entity_type = "NOUN_PHRASE".to_string();
                } else {
                    current_entity_text.push(' ');
                }
                current_entity_text.push_str(token);
            } else {
                if !current_entity_text.is_empty() {
                    entities.push(ExtractedEntity {
                        text: clean_token_text(&current_entity_text),
                        category: current_entity_type.clone(),
                    });
                    current_entity_text.clear();
                    current_entity_type.clear();
                }
            }
        }

        if !current_entity_text.is_empty() {
            entities.push(ExtractedEntity {
                text: clean_token_text(&current_entity_text),
                category: current_entity_type,
            });
        }

        Ok(entities)
    }
}

fn clean_token_text(token_text: &str) -> String {
    // Reassemble WordPiece tokens (e.g. "play" + "##ing" -> "playing")
    token_text.replace(" ##", "").replace("##", "")
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedEntity {
    pub text: String,
    pub category: String, // e.g. "PER", "ORG", "LOC", "MISC"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::models::install;

    #[test]
    fn test_pos_model_initialization_if_exists() {
        let model_path = install::default_pos_model_path(None);
        let tokenizer_path = install::default_pos_tokenizer_path(None);

        if model_path.exists() && tokenizer_path.exists() {
            let model = PosModel::new(&model_path, &tokenizer_path).unwrap();
            let entities = model
                .extract_entities("John works at Google in Seattle")
                .unwrap();
            assert!(!entities.is_empty());
        }
    }
}
