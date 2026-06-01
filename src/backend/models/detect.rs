use anyhow::{Result, anyhow};
use ndarray::Array2;
use ort::{inputs, session::Session, value::Value};
use std::sync::Mutex;
use tokenizers::Tokenizer;

// The ONNX model and tokenizer are embedded in the binary for ultra-fast startup
const MODEL_BYTES: &[u8] = include_bytes!("../../../assets/intent_model.onnx");
const TOKENIZER_JSON: &str = include_str!("../../../assets/intent_tokenizer.json");

/// Intent classifier using TinyBERT ONNX model
pub struct TinyBertDetector {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl TinyBertDetector {
    /// Creates a new TinyBertDetector
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

    /// Evaluates a single sentence with the TinyBERT model to get the actionable probability.
    pub fn predict_sentence(&self, sentence: &str) -> Result<f32> {
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
        let mut session_guard = self.session.lock().map_err(|e| anyhow!("Mutex poisoned: {}", e))?;
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
