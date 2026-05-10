/// Final text chunk produced by the transcription worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptionChunk {
    /// Transcribed body text.
    pub body: String,
    /// Monotonic chunk index carried through from the audio pipeline.
    pub index: u64,
}

impl TranscriptionChunk {
    /// Create a chunk from text and its source index.
    pub fn new(body: String, index: u64) -> Self {
        Self { body, index }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_constructor_keeps_text_and_index() {
        let chunk = TranscriptionChunk::new("hello".to_string(), 7);

        assert_eq!(chunk.body, "hello");
        assert_eq!(chunk.index, 7);
    }
}
