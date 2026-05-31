use anyhow::{Result, anyhow};
use ndarray::Array2;
use ort::{inputs, session::Session, value::Value};
use std::sync::Mutex;
use tokenizers::Tokenizer;

// The ONNX model and tokenizer are embedded in the binary for ultra-fast startup
const MODEL_BYTES: &[u8] = include_bytes!("../../assets/intent_model.onnx");
const TOKENIZER_JSON: &str = include_str!("../../assets/intent_tokenizer.json");

/// Intent detector using TinyBERT ONNX model
pub struct IntentDetector {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl IntentDetector {
    /// Creates a new IntentDetector
    pub fn new() -> Result<Self> {
        let tokenizer = Tokenizer::from_bytes(TOKENIZER_JSON.as_bytes())
            .map_err(|e| anyhow!("Failed to load tokenizer from bytes: {}", e))?;

        let session = Session::builder()
            .map_err(|e| anyhow!("Session builder error: {}", e))?
            .with_intra_threads(1)
            .map_err(|e| anyhow!("Failed to set threads: {}", e))?
            .commit_from_memory(MODEL_BYTES)
            .map_err(|e| anyhow!("Failed to load model from memory: {}", e))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    /// Stage 1: Fast keyword/heuristic gate using a flexible scoring system
    fn is_heuristic_match(words: &[String], has_question_mark: bool) -> bool {
        // "tayori" is the wake word, always pass
        if words.iter().any(|w| w == "tayori") {
            return true;
        }

        let mut score = 0.0;

        if has_question_mark {
            score += 0.80;
        }

        // Direct command imperatives (Tier 1: instantly pass)
        let commands = [
            "explain",
            "clarify",
            "summarize",
            "elaborate",
            "tell",
            "show",
            "describe",
            "detail",
            "list",
            "introduce",
            "state",
        ];
        // 5W1H interrogatives (Tier 2: need auxiliary or question mark support)
        let interrogatives = ["who", "what", "where", "why", "how", "when"];
        // Helper verbs and polite markers (Tier 3)
        let helpers = [
            "can", "could", "would", "should", "is", "are", "do", "does", "did", "please", "help",
            "know",
        ];

        for word in words {
            let w_str = word.as_str();
            if commands.contains(&w_str) {
                score += 0.75;
            } else if interrogatives.contains(&w_str) {
                score += 0.45;
            } else if helpers.contains(&w_str) {
                score += 0.35;
            }
        }

        score >= 0.70
    }

    /// Full ML detection pipeline with sliding window and stripping layer.
    pub fn detect(&self, text: &str) -> Result<IntentResult> {
        // 1. Preprocessing / Stripping layer for multi-word fillers
        let cleaned = text.to_lowercase();
        let cleaned = cleaned
            .replace("you know", " ")
            .replace("sort of", " ")
            .replace("kind of", " ");

        // Check if a question mark is present anywhere in the raw text
        let has_question_mark = text.contains('?');

        // 2. Clean tokens and filter out standard hesitation/filler adverbs
        let filler_adverbs = ["um", "uh", "er", "basically", "actually", "literally"];
        let words: Vec<String> = cleaned
            .split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| c.is_ascii_punctuation())
                    .to_string()
            })
            .filter(|w| !w.is_empty() && !filler_adverbs.contains(&w.as_str()))
            .collect();

        if words.is_empty() {
            return Ok(IntentResult {
                is_actionable: false,
                score: 0.0,
                similarity: None,
            });
        }

        // 3. Sliding Window Parameters
        const WINDOW_SIZE: usize = 32;
        const STRIDE: usize = 16;

        let mut max_prob = 0.0;

        // 4. Slide window and evaluate chunks
        let mut i = 0;
        while i < words.len() {
            let end = (i + WINDOW_SIZE).min(words.len());
            let chunk_words = &words[i..end];

            if Self::is_heuristic_match(chunk_words, has_question_mark) {
                let chunk_text = chunk_words.join(" ");
                let prob = self.predict_sentence(&chunk_text)?;
                if prob > max_prob {
                    max_prob = prob;
                }
            }

            if end == words.len() {
                break;
            }
            i += STRIDE;
        }

        Ok(IntentResult {
            is_actionable: max_prob > 0.70,
            score: max_prob,
            similarity: None,
        })
    }

    /// Evaluates a single sentence with the TinyBERT model to get the actionable probability.
    fn predict_sentence(&self, sentence: &str) -> Result<f32> {
        let encoding = self
            .tokenizer
            .encode(sentence, true)
            .map_err(|e| anyhow!("Tokenizer error: {}", e))?;

        let input_ids = encoding
            .get_ids()
            .iter()
            .map(|&x| x as i64)
            .collect::<Vec<_>>();
        let attention_mask = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect::<Vec<_>>();
        let token_type_ids = encoding
            .get_type_ids()
            .iter()
            .map(|&x| x as i64)
            .collect::<Vec<_>>();

        self.predict_tensors(input_ids, attention_mask, token_type_ids)
    }

    /// Runs ONNX inference on pre-processed tokens to return the actionable probability.
    fn predict_tensors(
        &self,
        input_ids: Vec<i64>,
        attention_mask: Vec<i64>,
        token_type_ids: Vec<i64>,
    ) -> Result<f32> {
        let seq_len = input_ids.len();

        let input_ids_arr = Array2::from_shape_vec((1, seq_len), input_ids)?;
        let attention_mask_arr = Array2::from_shape_vec((1, seq_len), attention_mask)?;
        let token_type_ids_arr = Array2::from_shape_vec((1, seq_len), token_type_ids)?;

        let input_ids_tensor = Value::from_array(input_ids_arr)?;
        let attention_mask_tensor = Value::from_array(attention_mask_arr)?;
        let token_type_ids_tensor = Value::from_array(token_type_ids_arr)?;

        // Run Inference with Mutex lock
        let mut session_guard = self.session.lock().unwrap();
        let outputs = session_guard
            .run(inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor,
            ])
            .map_err(|e| anyhow!("Inference error: {}", e))?;

        // Extract logits
        let (_, data) = outputs["logits"].try_extract_tensor::<f32>()?;

        // logits is shape [1, 2].
        let noise_logit = data[0];
        let actionable_logit = data[1];

        // Softmax
        let max_logit = noise_logit.max(actionable_logit);
        let exp_noise = (noise_logit - max_logit).exp();
        let exp_actionable = (actionable_logit - max_logit).exp();
        let sum_exp = exp_noise + exp_actionable;

        Ok(exp_actionable / sum_exp)
    }
}

#[derive(Debug, Clone)]
pub struct IntentResult {
    pub is_actionable: bool,
    pub score: f32,
    pub similarity: Option<f32>,
}
