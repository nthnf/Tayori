# Tayori

Tayori is an early-stage local-first meeting/audio assistant for Linux.

The current prototype captures system audio, detects speech, and transcribes it locally with Whisper models through `whisper-rs` / `whisper.cpp`. It is currently a developer prototype, not a packaged desktop app yet.

## What works today

- Capture system/output audio on Linux through CPAL + PipeWire.
- Convert captured audio into mono 16 kHz frames for VAD/STT.
- Detect speech activity with Silero VAD.
- Transcribe speech locally with Whisper `ggml` models.
- Run a terminal STT demo.
- Record processed STT audio to a WAV file for debugging.

## Current limitations

- Linux only.
- The current capture path targets system/output audio, not the user's microphone.
- The app is not packaged yet; run it through Cargo or the release binary.
- Local transcription performance depends heavily on the selected Whisper model and GPU/CPU.
- The current demo is for development/testing and may print repeated partial transcription output.

## Repository layout

```txt
.
├── crates/
│   ├── audio/      # audio capture, resampling, rolling buffer, VAD, snapshot scheduling
│   ├── core/       # demo binaries and orchestration experiments
│   ├── detection/  # question/usefulness detection experiments
│   ├── llm/        # LLM provider integration experiments
│   ├── rag/        # retrieval/embedding experiments
│   ├── storage/    # persistence experiments
│   └── stt/        # Whisper model paths, STT jobs, queue, and engine
├── migration/      # database migrations
└── README.md
```

## System requirements

### Required

- Linux with PipeWire.
- Rust stable toolchain.
- Cargo.
- C/C++ build tools.
- CMake.
- Clang/libclang for bindgen-based native builds.
- PipeWire development libraries.
- PulseAudio/PipeWire Pulse compatibility libraries.

### Recommended

- Dedicated GPU for local Whisper inference.
- Vulkan runtime and headers if building `whisper-rs` with Vulkan support.
- 16 GB RAM or more for comfortable local development.
- A small English Whisper model for live/local testing.

## Arch / Omarchy setup

On Arch-based systems, start with:

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
  vulkan-headers \
  vulkan-icd-loader
```

For AMD GPUs, make sure your Mesa/Vulkan driver is installed. For example:

```bash
sudo pacman -S --needed mesa vulkan-radeon
```

Useful GPU monitoring tools:

```bash
sudo pacman -S --needed nvtop
```

Optional AMD-specific monitor:

```bash
paru -S amdgpu_top
```

## Whisper models

Tayori expects Whisper `ggml` model files under the app data directory.

Default path shape:

```txt
$XDG_DATA_HOME/tayori/models/whisper/<model-file>
```

If `XDG_DATA_HOME` is not set:

```txt
~/.local/share/tayori/models/whisper/<model-file>
```

Example:

```txt
~/.local/share/tayori/models/whisper/ggml-small.en.bin
```

The STT crate default currently points at:

```txt
ggml-large-v3-turbo-q5_0.bin
```

Some demos may override this in code. For practical local testing, `small.en` or another small English model is recommended.

## Build

```bash
cargo build --release -p core
```

## Run STT demo

With logs:

```bash
target/release/stt_demo
```

Transcript-only mode:

```bash
QUIET=1 target/release/stt_demo
```

Stop with:

```txt
Ctrl+C
```

## Record processed STT audio

This records the same processed audio stream used by VAD/STT: mono, 16 kHz, 16-bit WAV.

```bash
cargo run --release -p core --bin capture_wav_demo
```

Custom duration:

```bash
cargo run --release -p core --bin capture_wav_demo -- --seconds 30
```

Custom output path:

```bash
cargo run --release -p core --bin capture_wav_demo -- --out /tmp/tayori-test.wav
```

Play the output:

```bash
pw-play /tmp/tayori-test.wav
```

This is meant for debugging what Whisper receives. It is not full-quality meeting playback audio.

## Development notes

The audio crate currently depends on CPAL from a pinned Git revision with `pipewire` and `pulseaudio` features enabled.

The STT crate currently uses `whisper-rs` with Vulkan support enabled.

The current performance bottleneck is local Whisper inference, not the Rust audio pipeline. Smaller English models are much more practical for low-latency local testing than medium/large models.

## License

Not specified yet.
