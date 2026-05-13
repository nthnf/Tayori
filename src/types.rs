#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Project,
    LiveRecording,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectDraft {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectCard {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentCard {
    pub id: String,
    pub name: String,
    pub meta: String,
    pub kind: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCard {
    pub id: String,
    pub title: String,
    pub status: String,
    pub meta: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptCard {
    pub id: String,
    pub text: String,
    pub time: String,
    pub chunk_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub has_question: bool,
    pub confidence: u8,
}
