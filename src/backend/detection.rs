use crate::backend::models::detect::TinyBertDetector;
use anyhow::Result;

/// Intent detector wrapper containing parsing logic, sliding window, and heuristics.
pub struct IntentDetector {
    model: TinyBertDetector,
}

impl IntentDetector {
    /// Creates a new IntentDetector wrapping the model
    pub fn new() -> Result<Self> {
        let model = TinyBertDetector::new()?;
        Ok(Self { model })
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
        let mut max_prob = 0.0;
        let mut best_sentence = None;

        // 1. Split text into logical sentences
        let sentences: Vec<&str> = text
            .split(['.', '?', '!'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        for sentence in sentences {
            let has_question_mark = text.contains('?'); // Alternatively, we could check if the original text had a question mark near this sentence

            // 2. Preprocessing / Stripping layer for multi-word fillers
            let cleaned = sentence.to_lowercase();
            let cleaned = cleaned
                .replace("you know", " ")
                .replace("sort of", " ")
                .replace("kind of", " ");

            // 3. Clean tokens and filter out standard hesitation/filler adverbs
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
                continue;
            }

            // 4. Evaluate chunk
            if Self::is_heuristic_match(&words, has_question_mark) {
                let chunk_text = words.join(" ");
                let prob = self.model.predict_sentence(&chunk_text)?;
                if prob > max_prob {
                    max_prob = prob;
                    best_sentence = Some(sentence.to_string());
                }
            }
        }

        let is_actionable = max_prob > 0.70;

        Ok(IntentResult {
            is_actionable,
            score: max_prob,
            similarity: None,
            extracted_query: if is_actionable { best_sentence } else { None },
        })
    }
}

#[derive(Debug, Clone)]
pub struct IntentResult {
    pub is_actionable: bool,
    pub score: f32,
    pub similarity: Option<f32>,
    pub extracted_query: Option<String>,
}
