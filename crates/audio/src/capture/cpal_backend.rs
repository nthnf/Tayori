use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, anyhow};
use cpal::{
    SampleFormat,
    traits::{DeviceTrait, StreamTrait},
};
use crossbeam_channel::bounded;
use ringbuf::{HeapRb, traits::*};
use tracing::{info, warn};

use super::{
    config::CpalCaptureConfig,
    device::{display_device_name, select_device, select_stream_config},
    handle::CpalCaptureHandle,
    ring_reader::{RingReaderConfig, spawn_ring_reader},
    sample_convert::interleaved_to_mono_f32,
};

pub(crate) fn start_cpal_capture(config: CpalCaptureConfig) -> Result<CpalCaptureHandle> {
    // Important:
    // With CPAL git + features = ["pipewire"], force the native PipeWire host.
    // Do not use default_host(), because on your machine it picked ALSA.
    let host = cpal::host_from_id(cpal::HostId::PipeWire)
        .context("failed to create CPAL PipeWire host")?;

    let device = select_device(&host, &config.device)?;
    let device_name = display_device_name(&device).unwrap_or_else(|_| "unknown device".to_string());

    let supported_config = select_stream_config(&device)?;
    let sample_format = supported_config.sample_format();

    let stream_config: cpal::StreamConfig = supported_config.clone().into();

    let input_sample_rate = stream_config.sample_rate;
    let input_channels = stream_config.channels as usize;

    let ring_capacity = input_sample_rate as usize * config.ring_seconds;

    let output_frame_samples = config.target_sample_rate as usize * config.output_frame_ms / 1000;

    if output_frame_samples == 0 {
        return Err(anyhow!("output_frame_ms produced zero samples"));
    }

    info!(
        host = ?host.id(),
        device = %device_name,
        input_sample_rate,
        input_channels,
        sample_format = ?sample_format,
        target_sample_rate = config.target_sample_rate,
        output_frame_ms = config.output_frame_ms,
        output_frame_samples,
        ring_capacity,
        "starting CPAL capture"
    );

    let ring = HeapRb::<f32>::new(ring_capacity);
    let (producer, consumer) = ring.split();

    let (frame_tx, frame_rx) = bounded(config.frame_channel_capacity);

    let stop = Arc::new(AtomicBool::new(false));

    let reader_thread = spawn_ring_reader(
        consumer,
        frame_tx,
        stop.clone(),
        RingReaderConfig {
            input_sample_rate,
            target_sample_rate: config.target_sample_rate,
            output_frame_samples,
        },
    );

    let stream = match sample_format {
        SampleFormat::F32 => build_input_stream::<f32, _>(
            &device,
            stream_config,
            input_channels,
            producer,
            stop.clone(),
        )?,

        SampleFormat::I16 => build_input_stream::<i16, _>(
            &device,
            stream_config,
            input_channels,
            producer,
            stop.clone(),
        )?,

        SampleFormat::U16 => build_input_stream::<u16, _>(
            &device,
            stream_config,
            input_channels,
            producer,
            stop.clone(),
        )?,

        other => {
            return Err(anyhow!(
                "unsupported CPAL sample format for first pass: {other:?}"
            ));
        }
    };

    stream.play().context("failed to start CPAL stream")?;

    info!("CPAL capture started");

    Ok(CpalCaptureHandle::new(
        frame_rx,
        stop,
        stream,
        reader_thread,
    ))
}

fn build_input_stream<T, P>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mut producer: P,
    stop: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    T: cpal::Sample + cpal::SizedSample + Copy + Send + 'static,
    f32: cpal::FromSample<T>,
    P: Producer<Item = f32> + Send + 'static,
{
    let mut mono_buffer = Vec::<f32>::with_capacity(4096);
    let mut dropped_samples = 0_u64;

    let stream = device.build_input_stream(
        config,
        move |data: &[T], _callback_info| {
            if stop.load(Ordering::Relaxed) {
                return;
            }

            mono_buffer.clear();

            interleaved_to_mono_f32(data, channels, &mut mono_buffer);

            for sample in mono_buffer.iter().copied() {
                if producer.try_push(sample).is_err() {
                    dropped_samples += 1;

                    if dropped_samples.is_multiple_of(16_000) {
                        warn!(dropped_samples, "audio ring buffer full; dropping samples");
                    }
                }
            }
        },
        move |err| {
            warn!(?err, "CPAL stream error");
        },
        None,
    )?;

    Ok(stream)
}
