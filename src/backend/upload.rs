use anyhow::{Result, anyhow};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::borrow::Cow;
use std::io::{Cursor, Read};

/// Processes an uploaded document by parsing its contents and chunking it.
/// Returns a list of chunks as strings.
pub fn process_document(bytes: &[u8], extension: &str) -> Result<Vec<String>> {
    let parsed_text = parse_file(bytes, extension)?;
    let chunks = chunk_text(&parsed_text)?;
    Ok(chunks)
}

/// Parses file bytes into raw text based on the file extension.
pub fn parse_file<'a>(bytes: &'a [u8], extension: &str) -> Result<Cow<'a, str>> {
    match extension.to_lowercase().as_str() {
        "txt" | "md" | "json" | "csv" | "rs" | "toml" => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| anyhow!("Failed to read file as UTF-8 text"))?;
            Ok(Cow::Borrowed(text))
        }
        "pdf" => {
            let text = pdf_extract::extract_text_from_mem(bytes)
                .map_err(|e| anyhow!("Failed to extract text from PDF: {}", e))?;
            Ok(Cow::Owned(text))
        }
        "docx" => {
            let text = parse_docx(bytes)?;
            Ok(Cow::Owned(text))
        }
        _ => Err(anyhow!("Unsupported file extension: {}", extension)),
    }
}

fn parse_docx(bytes: &[u8]) -> Result<String> {
    let reader = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| anyhow!("Failed to open DOCX as ZIP: {}", e))?;

    let mut document_xml = archive
        .by_name("word/document.xml")
        .map_err(|e| anyhow!("Failed to find word/document.xml in DOCX: {}", e))?;

    let mut xml_content = String::new();
    document_xml
        .read_to_string(&mut xml_content)
        .map_err(|e| anyhow!("Failed to read DOCX XML: {}", e))?;

    let mut reader = Reader::from_str(&xml_content);
    reader.config_mut().trim_text(false);

    let mut text = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"w:p" && !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            Ok(Event::Text(e)) => {
                let unescaped = String::from_utf8_lossy(e.as_ref());
                text.push_str(&unescaped);
            }
            Ok(Event::Eof) => break,
            Err(_) => break, // Or we could return an error, but DOCX often has quirks we can ignore
            _ => (),
        }
        buf.clear();
    }

    Ok(text)
}

/// Chunks the given text into smaller semantic pieces using the `chunk` crate.
pub fn chunk_text(text: &str) -> Result<Vec<String>> {
    let chunker = chunk::chunk(text.as_bytes());

    let mut chunks = Vec::new();
    for chunk_bytes in chunker {
        if let Ok(s) = std::str::from_utf8(chunk_bytes) {
            let s = s.trim();
            if !s.is_empty() {
                chunks.push(s.to_string());
            }
        }
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_document_success() {
        let content = b"This is the first sentence. This is the second sentence.";
        let chunks = process_document(content, "txt").unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_process_document_unsupported() {
        let content = b"binary data";
        let err = process_document(content, "exe").unwrap_err();
        assert_eq!(err.to_string(), "Unsupported file extension: exe");
    }

    #[test]
    fn test_parse_text_file() {
        let content = b"Hello world!";
        let text = parse_file(content, "txt").unwrap();
        assert_eq!(text, "Hello world!");
    }

    #[test]
    fn test_unsupported_file() {
        let content = b"binary data";
        let err = parse_file(content, "exe").unwrap_err();
        assert_eq!(err.to_string(), "Unsupported file extension: exe");
    }

    #[test]
    fn test_chunk_text_basic() {
        let text =
            "This is the first sentence. And this is the second sentence. Here comes a third one.";
        let chunks = chunk_text(text).unwrap();
        assert!(!chunks.is_empty(), "Chunks should not be empty");
        for chunk in &chunks {
            assert!(!chunk.is_empty(), "Individual chunks should not be empty");
        }
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("").unwrap();
        assert!(chunks.is_empty(), "Empty string should yield no chunks");
    }

    #[test]
    fn test_chunk_text_whitespace() {
        let chunks = chunk_text("   \n  \t  ").unwrap();
        assert!(
            chunks.is_empty(),
            "Whitespace string should yield no chunks"
        );
    }

    #[test]
    fn test_parse_pdf_invalid() {
        let content = b"Not a real PDF";
        let err = parse_file(content, "pdf");
        assert!(err.is_err(), "Invalid PDF bytes should return an error");
    }

    #[test]
    fn test_parse_docx_invalid() {
        let content = b"Not a real DOCX zip archive";
        let err = parse_file(content, "docx");
        assert!(err.is_err(), "Invalid DOCX bytes should return an error");
    }
}
