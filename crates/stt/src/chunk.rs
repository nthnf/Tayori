/// Final text chunk produced by the transcription worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptionChunk {
    /// Transcribed body text.
    pub body: String,
    /// Monotonic chunk index carried through from the audio pipeline.
    pub index: u64,
    /// Stream-relative source speech start time.
    pub start_ms: u64,
    /// Stream-relative source speech end time.
    pub end_ms: u64,
    /// Source speech duration.
    pub duration_ms: u64,
}

impl TranscriptionChunk {
    /// Create a chunk from text and its source index.
    pub fn new(body: String, index: u64, start_ms: u64, end_ms: u64) -> Self {
        Self {
            body,
            index,
            start_ms,
            end_ms,
            duration_ms: end_ms.saturating_sub(start_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_constructor_keeps_text_and_index() {
        let chunk = TranscriptionChunk::new("hello".to_string(), 7, 1_000, 2_500);

        assert_eq!(chunk.body, "hello");
        assert_eq!(chunk.index, 7);
        assert_eq!(chunk.start_ms, 1_000);
        assert_eq!(chunk.end_ms, 2_500);
        assert_eq!(chunk.duration_ms, 1_500);
    }
}
