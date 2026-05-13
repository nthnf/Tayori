# Tayori

Tayori is a local-first desktop assistant for live meetings and project documents.

It records local audio, transcribes speech with a local Whisper model, detects questions in the transcript, searches your project documents/transcript summaries, and streams a suggested answer from an OpenAI-compatible LLM. Project data is stored locally in SQLite; retrieval indexes are stored locally in LanceDB; the LLM API key is stored in your OS keyring.

Tayori is still early software. It is currently aimed at Linux users who are comfortable building from source.

## Features

- Desktop UI built with Dioxus.
- Local speech-to-text with Whisper `ggml` models.
- Voice activity detection with Silero VAD.
- Local SQLite database for projects, sessions, documents, transcripts, and answers.
- LanceDB-backed hybrid retrieval for project documents and transcript summaries.
- OpenAI-compatible answer generation.
- OS keyring storage for the single configured LLM API key.
- Local document upload for text-like files: `txt`, `md`, `markdown`, `csv`, `json`.

## Current Limits

- Linux is the primary supported platform right now.
- Packaging is not finished; build and run from source.
- Live audio/STT depends on your system audio setup and local Whisper model performance.
- Document ingestion currently expects UTF-8 text-like files.
- The LLM provider is currently OpenAI-compatible only.

## System Dependencies

You need:

- Rust stable toolchain and Cargo.
- C/C++ build tools.
- CMake.
- Clang and libclang for native Rust crates that use bindgen.
- PipeWire/PulseAudio compatibility libraries for audio capture.
- WebKitGTK/GTK libraries for the desktop webview.
- Secret Service/keyring support for API key storage.
- `curl` for downloading Whisper models through the bundled installer script.

On Arch Linux, this is a practical starting point:

```bash
sudo pacman -S --needed \
  base-devel \
  rust \
  cmake \
  clang \
  pipewire \
  pipewire-pulse \
  pipewire-alsa \
  alsa-lib \
  webkit2gtk-4.1 \
  gtk3 \
  libsecret \
  curl
```

For GPU/Vulkan-enabled native dependencies, also install your Vulkan runtime. For AMD on Arch:

```bash
sudo pacman -S --needed mesa vulkan-radeon vulkan-headers vulkan-icd-loader
```

Package names vary by distribution. On Debian/Ubuntu, look for equivalents such as `build-essential`, `cmake`, `clang`, `libclang-dev`, `libpipewire-0.3-dev`, `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, and `libsecret-1-dev`.

## Install Rust

If Rust is not installed, use rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

## Get The Source

```bash
git clone <repo-url> tayori
cd tayori
```

## Install A Whisper Model

Tayori uses local Whisper `ggml` model files stored under:

```txt
$XDG_DATA_HOME/tayori/models/whisper
```

If `XDG_DATA_HOME` is not set, the default is:

```txt
~/.local/share/tayori/models/whisper
```

Install the default practical model:

```bash
scripts/install-model.sh download --model small-q8_0
```

List available models:

```bash
scripts/install-model.sh list
```

The app can also trigger this installer from Rust when you change the Whisper model in Settings.

## Build

Debug build:

```bash
cargo build
```

Release build:

```bash
cargo build --release
```

The release binary will be at:

```txt
target/release/tayori
```

## Run

From source:

```bash
cargo run
```

Or after release build:

```bash
target/release/tayori
```

## First Run

1. Open Settings.
2. Click `Manage` for the API key.
3. Add your OpenAI-compatible API key.
4. Confirm the Whisper model is installed, for example `small-q8_0`.
5. Create a project.
6. Upload text-like documents if you want project context.
7. Create a live session and start recording.

The API key is stored in your OS keyring, not plaintext SQLite. Tayori stores only a marker indicating that the key belongs in the keyring.

## Data Locations

Tayori stores app data under your platform app-data directory. On Linux this is usually:

```txt
~/.local/share/tayori
```

Typical contents:

```txt
~/.local/share/tayori/db/          # SQLite and LanceDB data
~/.local/share/tayori/models/      # local Whisper models
```

Uploaded documents are not copied for the current MVP. Tayori stores the original path, document metadata, chunk text, and retrieval vectors.

## Developer Checks

Run tests for the core orchestration layer:

```bash
cargo test -p core
```

Run workspace clippy:

```bash
cargo clippy --workspace --all-targets
```

Run a full check:

```bash
cargo check
```
