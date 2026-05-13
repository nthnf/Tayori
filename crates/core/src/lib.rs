//! Core orchestration layer for Tayori.
//!
//! This crate is intentionally the only layer the Dioxus UI should call for
//! application behavior. `storage` owns SQLite and LanceDB connections, but UI
//! code must not import storage entities directly because that leaks database
//! representation details into rendering code and makes it easy to bypass the
//! single bootstrapped `TayoriCore` instance.
//!
//! Responsibilities owned here:
//! - bootstrap storage and migrations;
//! - expose UI-safe DTOs instead of SeaORM entities;
//! - orchestrate document upload, embedding, hybrid retrieval, reranking, and
//!   LLM answer generation;
//! - persist transcript chunks and map detector output into UI state;
//! - keep secret handling behind OS keyring helpers.
//!
//! Responsibilities deliberately not owned here:
//! - Dioxus signal state and visual layout;
//! - raw LanceDB Arrow batch construction;
//! - low-level audio capture, VAD, and Whisper decoding internals.

pub mod answer;
pub mod audio_runtime;
pub mod project;
pub mod retrieval;
pub mod service;
pub mod session;
pub mod settings;
pub mod transcript;
pub mod upload;

pub use audio_runtime::AudioRuntime;
pub use service::TayoriCore;
