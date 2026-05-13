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
    pub id: usize,
    pub name: String,
    pub description: String,
}
