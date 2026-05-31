/// Greedy autoregressive token selection with repetition detection.
///
/// Wraps the common argmax + EOS + repeat-guard pattern shared by all
/// autoregressive decoder engines (Canary, Moonshine, Cohere).
///
/// Each engine still owns its KV cache and decoder session — this struct
/// only handles token selection and stopping decisions.
const DEFAULT_MAX_CONSECUTIVE_REPEATS: usize = 4;
const MAX_NGRAM_SIZE: usize = 4;

pub struct GreedyDecoder {
    eos_id: i64,
    max_consecutive_repeats: usize,
    history: Vec<i64>,
}

impl GreedyDecoder {
    pub fn new(eos_id: i64) -> Self {
        Self {
            eos_id,
            max_consecutive_repeats: DEFAULT_MAX_CONSECUTIVE_REPEATS,
            history: Vec::with_capacity(128),
        }
    }

    pub fn with_max_repeats(mut self, n: usize) -> Self {
        self.max_consecutive_repeats = n;
        self
    }

    /// Given logits for the last decoder position, pick the next token.
    ///
    /// Returns `Some(token_id)` to continue decoding, or `None` to stop
    /// (EOS reached or repetition limit hit).
    pub fn next_token(&mut self, logits: &[f32]) -> Option<i64> {
        let token = argmax(logits) as i64;

        if token == self.eos_id {
            return None;
        }

        self.history.push(token);

        if has_ngram_repeats(&self.history, MAX_NGRAM_SIZE, self.max_consecutive_repeats) {
            log::warn!(
                "Greedy decode: N-gram repeated {} consecutive times, stopping",
                self.max_consecutive_repeats
            );
            return None;
        }

        Some(token)
    }
}

fn has_ngram_repeats(history: &[i64], max_n: usize, required_repeats: usize) -> bool {
    let len = history.len();
    for n in 1..=max_n {
        let total_len = n * required_repeats;
        if len >= total_len {
            let mut is_repeat = true;
            let target_ngram = &history[len - n..len];
            for i in 1..required_repeats {
                let check_ngram = &history[len - (i + 1) * n..len - i * n];
                if check_ngram != target_ngram {
                    is_repeat = false;
                    break;
                }
            }
            if is_repeat {
                return true;
            }
        }
    }
    false
}

fn argmax(logits: &[f32]) -> usize {
    let mut max_idx = 0;
    let mut max_val = f32::NEG_INFINITY;
    for (i, &v) in logits.iter().enumerate() {
        if v > max_val {
            max_val = v;
            max_idx = i;
        }
    }
    max_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argmax() {
        assert_eq!(argmax(&[1.0, 3.0, 2.0]), 1);
        assert_eq!(argmax(&[-1.0, -3.0, -0.5]), 2);
        assert_eq!(argmax(&[5.0]), 0);
    }

    #[test]
    fn test_eos_stops() {
        let mut dec = GreedyDecoder::new(2);
        // logits where token 2 (EOS) wins
        assert_eq!(dec.next_token(&[0.0, 0.0, 10.0, 0.0]), None);
    }

    #[test]
    fn test_normal_token() {
        let mut dec = GreedyDecoder::new(2);
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0, 0.0]), Some(1));
    }

    #[test]
    fn test_repeat_limit() {
        let mut dec = GreedyDecoder::new(99).with_max_repeats(3);
        let logits = [0.0, 10.0, 0.0]; // always picks token 1
        assert_eq!(dec.next_token(&logits), Some(1)); // length 1
        assert_eq!(dec.next_token(&logits), Some(1)); // length 2
        assert_eq!(dec.next_token(&logits), None); // length 3 -> hits repeat limit
    }

    #[test]
    fn test_repeat_resets_on_different_token() {
        let mut dec = GreedyDecoder::new(99).with_max_repeats(3);
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[10.0, 0.0, 0.0]), Some(0));
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), None); // Stop at 3 repeats of token 1
    }

    #[test]
    fn test_ngram_repeat() {
        let mut dec = GreedyDecoder::new(99).with_max_repeats(3);
        // N-gram: [1, 2]
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[0.0, 0.0, 10.0]), Some(2));
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[0.0, 0.0, 10.0]), Some(2));
        assert_eq!(dec.next_token(&[0.0, 10.0, 0.0]), Some(1));
        assert_eq!(dec.next_token(&[0.0, 0.0, 10.0]), None); // Stop at 3 repeats of [1, 2]
    }

    #[test]
    fn test_nan_handling() {
        let mut dec = GreedyDecoder::new(99);
        // NaN logits — argmax uses `>` which is false for NaN, so index 0 wins
        assert_eq!(dec.next_token(&[f32::NAN, f32::NAN, f32::NAN]), Some(0));
    }
}
