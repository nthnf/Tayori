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
    pub meta: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsForm {
    pub llm_provider: String,
    pub llm_model: String,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub sparse_model: String,
    pub reranker_model: String,
    pub whisper_model: String,
    pub summary_minutes: String,
    pub ui_theme: Theme,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptCard {
    pub id: String,
    pub text: String,
    pub time: String,
}
