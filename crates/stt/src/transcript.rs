#[derive(Debug, Clone)]
pub struct Transcription {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    /// Whisper timestamp unit is centiseconds.
    pub start_cs: i64,

    /// Whisper timestamp unit is centiseconds.
    pub end_cs: i64,

    pub text: String,
}

impl TranscriptSegment {
    pub fn start_ms(&self) -> i64 {
        self.start_cs * 10
    }

    pub fn end_ms(&self) -> i64 {
        self.end_cs * 10
    }
}
