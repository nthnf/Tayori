/// Audio source kind for capture setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioSource {
    /// System/call audio path.
    Monitor,
    /// User microphone path.
    Mic,
}
