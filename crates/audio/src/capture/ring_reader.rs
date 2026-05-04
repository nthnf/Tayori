use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam_channel::Sender;
use ringbuf::traits::Consumer;
use tracing::{info, warn};

use crate::AudioFrame;

use super::resampler::RubatoResampler;

pub(crate) struct RingReaderConfig {
    pub input_sample_rate: u32,
    pub target_sample_rate: u32,
    pub output_frame_samples: usize,
}

pub(crate) fn spawn_ring_reader<C>(
    mut consumer: C,
    frame_tx: Sender<AudioFrame>,
    stop: Arc<AtomicBool>,
    config: RingReaderConfig,
) -> thread::JoinHandle<()>
where
    C: Consumer<Item = f32> + Send + 'static,
{
    thread::Builder::new()
        .name("tayori-audio-ring-reader".to_string())
        .spawn(move || {
            info!(
                input_sample_rate = config.input_sample_rate,
                target_sample_rate = config.target_sample_rate,
                output_frame_samples = config.output_frame_samples,
                "audio ring reader started"
            );

            let mut resampler =
                match RubatoResampler::new(config.input_sample_rate, config.target_sample_rate) {
                    Ok(resampler) => resampler,
                    Err(err) => {
                        tracing::error!(?err, "failed to initialize Rubato resampler");
                        return;
                    }
                };

            let mut input_batch = Vec::<f32>::with_capacity(2048);
            let mut resampled = Vec::<f32>::with_capacity(2048);
            let mut frame = Vec::<f32>::with_capacity(config.output_frame_samples);

            let mut dropped_frames = 0_u64;

            while !stop.load(Ordering::Relaxed) {
                input_batch.clear();

                while let Some(sample) = consumer.try_pop() {
                    input_batch.push(sample);

                    if input_batch.len() >= 2048 {
                        break;
                    }
                }

                if input_batch.is_empty() {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }

                resampled.clear();

                if let Err(err) = resampler.push(&input_batch, &mut resampled) {
                    warn!(?err, "failed to resample audio batch");
                    continue;
                }

                for sample in resampled.drain(..) {
                    frame.push(sample);

                    if frame.len() >= config.output_frame_samples {
                        let samples = std::mem::replace(
                            &mut frame,
                            Vec::with_capacity(config.output_frame_samples),
                        );

                        let audio_frame = AudioFrame::mono_16k(samples);

                        if let Err(err) = frame_tx.try_send(audio_frame) {
                            dropped_frames += 1;

                            if dropped_frames.is_multiple_of(100) {
                                warn!(
                                    ?err,
                                    dropped_frames,
                                    "processed audio frame channel full; dropping frames"
                                );
                            }
                        }
                    }
                }
            }

            info!("audio ring reader stopped");
        })
        .expect("failed to spawn audio ring reader")
}
