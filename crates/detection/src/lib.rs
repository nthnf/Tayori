const QUESTION_STARTERS: &[&str] = &[
    "who", "what", "where", "when", "why", "how", "can", "could", "would", "should", "do", "does",
    "did", "is", "are", "am", "was", "were", "have", "has", "will",
];

const EMBEDDED_QUESTION_PATTERNS: &[&str] = &[
    "i wonder",
    "do you know",
    "can anyone tell",
    "any idea",
    "does anyone know",
];

const COMMAND_PATTERNS: &[&str] = &[
    "please",
    "can you",
    "could you",
    "would you",
    "let's",
    "lets",
    "we need to",
    "i need you to",
    "make sure",
    "follow up",
    "send",
    "schedule",
    "remind",
    "add",
    "create",
    "open",
    "show",
    "pull up",
    "look into",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub has_question: bool,
    pub has_command: bool,
    pub confidence: f32,
    pub reason: String,
}

pub fn detect_question_or_command(chunk: &str) -> Detection {
    let text = normalize(chunk);
    if text.is_empty() {
        return Detection {
            has_question: false,
            has_command: false,
            confidence: 0.0,
            reason: "empty".to_string(),
        };
    }

    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let first = tokens.first().copied().unwrap_or_default();

    let has_question_mark = chunk.contains('?');
    let starts_like_question = QUESTION_STARTERS.contains(&first);
    let has_embedded_question = EMBEDDED_QUESTION_PATTERNS
        .iter()
        .any(|pattern| has_phrase(&text, pattern));
    let has_command = COMMAND_PATTERNS
        .iter()
        .any(|pattern| has_phrase(&text, pattern));
    let has_question = has_question_mark || starts_like_question || has_embedded_question;

    let mut confidence: f32 = 0.2;
    if has_question_mark {
        confidence += 0.4;
    }
    if starts_like_question {
        confidence += 0.3;
    }
    if has_embedded_question {
        confidence += 0.25;
    }
    if has_command {
        confidence += 0.35;
    }

    let mut reasons = Vec::new();
    if has_question_mark {
        reasons.push("question_mark");
    }
    if starts_like_question {
        reasons.push("question_starter");
    }
    if has_embedded_question {
        reasons.push("embedded_question");
    }
    if has_command {
        reasons.push("command_pattern");
    }

    Detection {
        has_question,
        has_command,
        confidence: confidence.min(1.0),
        reason: if reasons.is_empty() {
            "weak/no signal".to_string()
        } else {
            reasons.join(", ")
        },
    }
}

fn normalize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() || ch == '\'' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_phrase(text: &str, phrase: &str) -> bool {
    if phrase.split_whitespace().count() == 1 {
        text.split_whitespace().any(|token| token == phrase)
    } else {
        text.contains(phrase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_empty_text() {
        let detection = detect_question_or_command("   ");

        assert!(!detection.has_question);
        assert!(!detection.has_command);
        assert_eq!(detection.confidence, 0.0);
        assert_eq!(detection.reason, "empty");
    }

    #[test]
    fn detects_question_mark() {
        let detection = detect_question_or_command("We can ship this today?");

        assert!(detection.has_question);
        assert!(!detection.has_command);
        assert!(detection.reason.contains("question_mark"));
    }

    #[test]
    fn detects_question_starter() {
        let detection = detect_question_or_command("How should we handle retries");

        assert!(detection.has_question);
        assert!(detection.reason.contains("question_starter"));
    }

    #[test]
    fn detects_embedded_question() {
        let detection = detect_question_or_command("I wonder if the customer needs this");

        assert!(detection.has_question);
        assert!(detection.reason.contains("embedded_question"));
    }

    #[test]
    fn detects_command() {
        let detection = detect_question_or_command("Please follow up with the team");

        assert!(!detection.has_question);
        assert!(detection.has_command);
        assert!(detection.reason.contains("command_pattern"));
    }

    #[test]
    fn caps_confidence() {
        let detection = detect_question_or_command("Can you please show the roadmap?");

        assert!(detection.has_question);
        assert!(detection.has_command);
        assert_eq!(detection.confidence, 1.0);
    }

    #[test]
    fn avoids_single_word_substring_false_positive() {
        let detection = detect_question_or_command("The openai model is ready");

        assert!(!detection.has_command);
    }
}
