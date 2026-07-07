use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig, SupportedStreamConfig};
use rubato::{FftFixedInOut, Resampler};
use std::collections::VecDeque;
use std::f32::consts::PI;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    println!("=== MoFA Audio Config Probe ===");
    println!("Goal: dynamically negotiate output config and play a resampled chunk.\n");

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("No output device found")?;

    println!("Output device: {}", device.name().unwrap_or_else(|_| "<unknown>".to_string()));

    let default_cfg = device
        .default_output_config()
        .context("Failed to query default output config")?;

    print_default_config(&default_cfg);
    print_supported_configs(&device)?;

    let target_rate = default_cfg.sample_rate().0;
    let channels = default_cfg.channels() as usize;

    let input_24k = make_sine_chunk(24_000, 0.3, 440.0);
    let resampled = if target_rate == 24_000 {
        input_24k.clone()
    } else {
        resample_24k_to_target(&input_24k, target_rate as usize)?
    };

    println!(
        "\nResample summary: 24k input frames={} -> target {}Hz frames={}",
        input_24k.len(),
        target_rate,
        resampled.len()
    );

    let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    {
        let mut q = queue.lock().expect("audio queue poisoned");
        for sample in &resampled {
            for _ in 0..channels {
                q.push_back(*sample);
            }
        }
        let silence_frames = target_rate as usize / 5;
        for _ in 0..(silence_frames * channels) {
            q.push_back(0.0);
        }
    }

    let stream_cfg = StreamConfig {
        channels: default_cfg.channels(),
        sample_rate: SampleRate(target_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let stream = build_stream_for_sample_format(
        &device,
        default_cfg.sample_format(),
        &stream_cfg,
        queue.clone(),
    )?;

    stream.play().context("Failed to start output stream")?;
    println!("\nStream started successfully. Playing probe tone for ~1 second...");
    thread::sleep(Duration::from_secs(1));
    drop(stream);

    println!("Success: dynamic negotiation + playback completed.");
    println!("\nUse this evidence in proposal: cpal default/supported config + runtime resample.");
    Ok(())
}

fn print_default_config(cfg: &SupportedStreamConfig) {
    println!("\nDefault output config:");
    println!("  channels: {}", cfg.channels());
    println!("  sample_rate: {}", cfg.sample_rate().0);
    println!("  sample_format: {:?}", cfg.sample_format());
    println!("  buffer_size: {:?}", cfg.buffer_size());
}

fn print_supported_configs(device: &cpal::Device) -> Result<()> {
    println!("\nSupported output config ranges:");
    let ranges = device
        .supported_output_configs()
        .context("Failed to enumerate supported output configs")?;

    let mut count = 0usize;
    for range in ranges {
        count += 1;
        println!(
            "  [{}] channels={} format={:?} min_rate={} max_rate={}",
            count,
            range.channels(),
            range.sample_format(),
            range.min_sample_rate().0,
            range.max_sample_rate().0
        );
    }

    if count == 0 {
        println!("  (No supported ranges reported)");
    }

    Ok(())
}

fn make_sine_chunk(sample_rate: usize, seconds: f32, freq_hz: f32) -> Vec<f32> {
    let frames = (sample_rate as f32 * seconds).round() as usize;
    (0..frames)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (2.0 * PI * freq_hz * t).sin() * 0.2
        })
        .collect()
}

fn resample_24k_to_target(input: &[f32], target_rate: usize) -> Result<Vec<f32>> {
    let in_rate = 24_000usize;
    let chunk_size = input.len();

    let mut resampler = FftFixedInOut::<f32>::new(in_rate, target_rate, chunk_size, 1)
        .with_context(|| format!("Failed to create rubato resampler {} -> {}", in_rate, target_rate))?;

    let input_channels = vec![input.to_vec()];
    let mut output_channels = resampler
        .process(&input_channels, None)
        .context("rubato processing failed")?;

    // FftFixedInOut outputs all samples in single process() call when given full input.
    // Pop the single output channel and return it.
    output_channels
        .pop()
        .context("rubato returned no output channel")
}

fn build_stream_for_sample_format(
    device: &cpal::Device,
    format: SampleFormat,
    config: &StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
) -> Result<cpal::Stream> {
    let err_fn = |e| eprintln!("Audio thread error: {e}");

    match format {
        SampleFormat::F32 => build_stream::<f32>(device, config, queue, err_fn),
        SampleFormat::I16 => build_stream::<i16>(device, config, queue, err_fn),
        SampleFormat::U16 => build_stream::<u16>(device, config, queue, err_fn),
        other => anyhow::bail!("Unsupported sample format: {other:?}"),
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;

    let stream = device
        .build_output_stream(
            config,
            move |output: &mut [T], _info| {
                let mut q = queue.lock().expect("audio queue poisoned");
                for frame in output.chunks_mut(channels) {
                    let sample_f32 = q.pop_front().unwrap_or(0.0);
                    let sample_t: T = T::from_sample(sample_f32);
                    for s in frame {
                        *s = sample_t;
                    }
                }
            },
            err_fn,
            None,
        )
        .context("Failed to build output stream with selected format")?;

    Ok(stream)
}

#[allow(dead_code)]
fn _wait_for_enter() {
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}
