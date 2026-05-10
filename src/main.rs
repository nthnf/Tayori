use dioxus::prelude::*;
use tayori_core::AudioRuntime;
use std::time::Duration;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut audio_runtime = use_signal(|| None::<AudioRuntime>);
    let mut status = use_signal(|| String::from("idle"));
    let transcript = use_signal(String::new);

    let on_start = move |_| {
        if audio_runtime.read().is_some() {
            status.set(String::from("already running"));
            return;
        }

        match AudioRuntime::start_default() {
            Ok(mut runtime) => {
                let transcript_rx = runtime.transcriptions();
                let mut transcript_signal = transcript;

                spawn(async move {
                    loop {
                        while let Ok(chunk) = transcript_rx.try_recv() {
                            transcript_signal.with_mut(|text| {
                                if !text.is_empty() {
                                    text.push('\n');
                                }
                                text.push_str(&chunk.body);
                            });
                        }

                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                });

                if let Err(err) = runtime.start() {
                    status.set(format!("start failed: {err}"));
                    return;
                }

                audio_runtime.set(Some(runtime));
                status.set(String::from("running"));
            }
            Err(err) => {
                status.set(format!("start failed: {err}"));
            }
        }
    };

    let on_stop = move |_| {
        if let Some(mut runtime) = audio_runtime.write().take() {
            runtime.stop();
        }

        status.set(String::from("stopped"));
    };

    rsx! {
        div {
            style: "max-width: 900px; margin: 0 auto; padding: 24px; font-family: sans-serif;",
            h1 { "Tayori" }
            p { "Status: {status}" }

            div {
                style: "display: flex; gap: 12px; margin: 16px 0;",
                button {
                    onclick: on_start,
                    "Start"
                }
                button {
                    onclick: on_stop,
                    "Stop"
                }
            }

            h2 { "Transcript" }
            pre {
                style: "min-height: 240px; padding: 12px; border: 1px solid #ccc; border-radius: 8px; white-space: pre-wrap;",
                "{transcript}"
            }
        }
    }
}
