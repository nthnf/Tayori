# Tayori

Tayori is a local-first meeting/audio assistant experiment.

The current prototype captures Linux system audio, detects speech, transcribes speech with local Whisper models through `whisper-rs`, and prints transcript output from a demo binary. The product direction is **not** a live transcript app. The goal is to capture meeting context, detect useful/question-like moments, and help answer or summarize with an efficient local/cloud AI pipeline.

## Current status

This repository is still early-stage. The current working pieces are:

- Linux system audio capture through CPAL/PipeWire.
- Audio normalization into mono 16 kHz frames for VAD/STT.
- Rolling audio buffer for recent STT windows.
- Silero VAD watcher for speech activity detection.
- Local Whisper STT through `whisper-rs` / `whisper.cpp`.
- STT job queue with latest-partial replacement and final job priority.
- Debug/demo binaries for WAV capture and STT timing.

The current README used to be the default Dioxus template; this file documents the actual Tayori direction.

## Workspace layout

```txt
.
├── crates/
│   ├── audio/      # capture, resampling, rolling buffer, VAD, STT snapshot scheduling
│   ├── core/       # demo binaries and orchestration experiments
│   ├── detection/  # planned question/usefulness detection
│   ├── llm/        # planned LLM provider abstraction
│   ├── rag/        # planned retrieval/embedding layer
│   ├── storage/    # planned session/project persistence
│   └── stt/        # Whisper model pathing, STT jobs, scheduler, engine
├── migration/      # database migrations
└── README.md
```

The root workspace currently includes `audio`, `core`, `detection`, `llm`, `rag`, `storage`, and `stt` crates.

## Current audio/STT flow

```txt
System audio
  ↓
CPAL PipeWire capture
  ↓
16 kHz mono AudioFrame stream
  ↓
RollingAudioBuffer
  ↓
SileroVadWatcher
  ↓
LiveSnapshotScheduler
  ↓
SttJobInbox
  ↓
WhisperEngine
  ↓
Transcript output
```

Important implementation details:

- Audio capture currently targets system/output audio, not the user's microphone.
- STT expects Whisper `ggml` model files under Tayori's app data directory.
- The STT crate default model path is:

```txt
$XDG_DATA_HOME/tayori/models/whisper/ggml-large-v3-turbo-q5_0.bin
```

If `XDG_DATA_HOME` is unset, it falls back to:

```txt
~/.local/share/tayori/models/whisper/ggml-large-v3-turbo-q5_0.bin
```

Demo binaries may override the model name directly in code.

## Running the STT demo

Build first:

```bash
cargo build --release -p core
```

Run with logs:

```bash
target/release/stt_demo
```

Run transcript-only mode:

```bash
QUIET=1 target/release/stt_demo
```

Stop the demo with `Ctrl+C`.

## Recording processed audio for debugging

The WAV demo records the same processed audio format used by VAD/STT: mono, 16 kHz, 16-bit WAV.

```bash
cargo run --release -p core --bin capture_wav_demo
```

Custom duration:

```bash
cargo run --release -p core --bin capture_wav_demo -- --seconds 30
```

Custom output:

```bash
cargo run --release -p core --bin capture_wav_demo -- --out /tmp/tayori-test.wav
```

This is useful for checking what Whisper actually receives. It is not meant to be full-quality meeting playback audio.

## Current performance findings

The prototype showed that Tayori's Rust/audio architecture is cheap compared to Whisper inference:

- Rolling buffer push/slice, queue operations, and segment collection are tiny.
- Whisper `state.full()` dominates STT job time.
- `small.en`-class models are practical for live-ish local use on a midrange GPU.
- Medium/large models are better suited for background repair/context, not always-on low-latency transcription.

This means the main optimization strategy should be **fewer, more valuable STT jobs**, not micro-optimizing the buffer or channels first.

## Target product architecture

Tayori should move from a live-transcript demo to a session timeline system.

```txt
System audio capture
  ├── Raw audio recorder lane
  │     └── save full meeting audio to disk
  │
  └── STT processing lane
        ↓
      VAD speech chunks
        ↓
      local Whisper small/en model
        ↓
      transcript chunks
        ↓
      question/usefulness detector
        ↓
      answer/context pipeline
```

The meeting record should not be only a transcript. It should be a timeline of events:

```txt
SessionTimeline
  ├── audio_chunk
  ├── transcript_chunk
  ├── question_candidate
  ├── assistant_answer
  ├── user_note
  ├── user_override_answer
  ├── answer_feedback
  └── summary_event
```

This matters because Tayori currently captures system audio, not the user's mic. If Tayori suggests an answer, or the user answers differently, that has to be stored as a timeline event or the meeting record will be incomplete.

## Planned architecture update

### 1. Stop treating live partials as the product

Live partials are useful for debugging, but they create repeated transcript spam and extra Whisper jobs.

Default app behavior should become:

```txt
VAD speech start
  ↓
collect speech chunk
  ↓
finalize after short silence or max chunk duration
  ↓
transcribe final chunk
```

Suggested defaults:

```txt
min chunk:          1.0s–1.5s
silence finalize:   600ms–800ms
normal chunk:        4s–8s
forced max chunk:    8s–12s
overlap:             500ms–1s
rolling buffer:      60s
```

Do not use 60-second Whisper jobs as the normal STT path. Keep the 60-second rolling buffer for recovery/context, not routine transcription.

### 2. Add raw audio recorder lane

Store full session audio directly to disk while keeping the STT rolling buffer small.

Recommended split:

```txt
Human replay recording:
  48 kHz stereo Opus/WebM or WAV

STT processing stream:
  16 kHz mono f32
```

The app should never keep the whole meeting audio in memory.

### 3. Store transcript chunks with timestamps

Minimal shape:

```rust
struct TranscriptChunk {
    session_id: String,
    start_ms: i64,
    end_ms: i64,
    text: String,
    is_question_candidate: bool,
}
```

This enables time-based context retrieval without embedding every chunk immediately.

### 4. Add cheap question/usefulness detection

Whisper cannot cheaply skim audio for questions. The practical MVP flow is:

```txt
audio → VAD → STT text → question detector
```

Start with a cheap text detector:

- question mark
- question words: what, why, how, when, where, can, could, should, would
- intent phrases: explain, compare, design, how do I, what if

Later this can become a small classifier or LLM-based detector.

### 5. Add meeting memory without embedding everything live

Do not tokenize/embed the whole meeting in real time.

Use layered memory:

```txt
Recent verbatim memory:
  last 60–120s transcript chunks

Rolling summary memory:
  compact topic summary updated less frequently

Persistent transcript store:
  all chunks with timestamps

Embeddings:
  question chunks immediately
  normal chunks lazily/background/after meeting
```

For a detected question, build context from:

```txt
current question chunk
+ previous 60–120s transcript
+ current rolling summary
+ optional retrieved chunks
```

### 6. Store assistant and user answer events

Because microphone capture is not currently part of the app, Tayori must store non-audio events:

```txt
assistant_answer
user_override_answer
user_note
answer_feedback
```

If the user rejects the LLM answer and uses their own response, store the user's response as the source of truth for later summaries.

### 7. Make compute modes explicit

Tayori should have modes instead of pretending every device can run everything locally.

Low-power mode:

```txt
STT: small/en model
partials: off
embeddings: question-only live
LLM: cloud/provider API
summary: after meeting only
```

Recommended mode:

```txt
STT: small/en model
partials: off by default
embeddings: question-only live, background for transcript
LLM: provider API or optional local
raw audio: enabled
```

High-accuracy mode:

```txt
STT: medium/large repair lane
embeddings: live + background
LLM: local optional
summaries: periodic/post-meeting
```

## Strong default direction

For the MVP, Tayori should default to:

```txt
raw audio recording:     on
live partial transcript: off
STT model:               small/en class
STT chunks:              final/forced chunks only
question detection:      cheap text detector
embeddings:              question-only live
LLM answer generation:   provider/API first
summarization:           optional after meeting
```

This keeps the app realistic on midrange machines and avoids turning every second of meeting audio into expensive AI work.
