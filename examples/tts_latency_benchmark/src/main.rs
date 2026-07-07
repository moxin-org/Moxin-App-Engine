use anyhow::{bail, Context, Result};
use futures::StreamExt;
use reqwest::StatusCode;
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use serde_json::json;
use std::env;
use std::time::{Duration, Instant};

const OPENAI_API_URL: &str = "https://api.openai.com/v1/audio/speech";
const DEFAULT_MODEL: &str = "tts-1";
const DEFAULT_VOICE: &str = "alloy";
const DEFAULT_SAMPLE_RATE: u32 = 24_000;

#[derive(Debug)]
struct BenchmarkResult {
    total_ms: u128,
    ttfb_ms: Option<u128>,
    first_audio_queue_ms: Option<u128>,
    bytes_received: usize,
}

#[derive(Debug)]
struct RunRecord {
    run: usize,
    full: BenchmarkResult,
    stream: BenchmarkResult,
}

#[derive(Debug, Clone, Copy)]
struct SummaryStats {
    mean_ms: f64,
    median_ms: u128,
    p95_ms: u128,
    min_ms: u128,
    max_ms: u128,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let api_key = env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is required")?;
    let model = env::var("TTS_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let voice = env::var("TTS_VOICE").unwrap_or_else(|_| DEFAULT_VOICE.to_string());
    let text = env::var("TTS_TEXT").unwrap_or_else(|_| {
        "This is a streaming latency benchmark for MoFA. We compare full download against first chunk playback.".to_string()
    });
    let save_full = env::var("SAVE_FULL_PCM").is_ok();
    let playback = env::var("AUDIO_PLAYBACK").map(|v| v != "0").unwrap_or(true);
    let runs = env::var("TTS_RUNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1);
    let csv_path = env::var("TTS_CSV").ok();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("Failed to build reqwest client")?;

    println!("=== TTS Streaming Latency Benchmark ===");
    println!("Model: {model}");
    println!("Voice: {voice}");
    println!("Text length: {}", text.len());
    println!("Playback: {}", if playback { "on" } else { "off" });
    println!("Runs: {runs}");
    println!();

    let mut records = Vec::with_capacity(runs);
    for run_idx in 0..runs {
        println!("----- Run {}/{} -----", run_idx + 1, runs);
        let full = run_full_download(
            &client,
            &api_key,
            &model,
            &voice,
            &text,
            playback,
            save_full && run_idx == 0,
        )
        .await?;
        let stream = run_streaming(&client, &api_key, &model, &voice, &text, playback).await?;

        print_summary(&full, &stream);
        records.push(RunRecord {
            run: run_idx + 1,
            full,
            stream,
        });
    }

    print_aggregate_summary(&records, playback);

    if let Some(path) = csv_path {
        write_csv(&path, &records, playback).await?;
        println!("CSV written: {path}");
    }

    Ok(())
}

async fn run_full_download(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    voice: &str,
    text: &str,
    playback: bool,
    save_full: bool,
) -> Result<BenchmarkResult> {
    println!("[A] Full download mode: wait for complete response then play");

    let start = Instant::now();
    let response = tts_request(client, api_key, model, voice, text).await?;
    let bytes = response
        .bytes()
        .await
        .context("Failed reading full TTS response body")?;
    let recv_done = start.elapsed();

    let mut first_audio_queue_ms = None;
    if playback {
        let play_start = Instant::now();
        play_pcm_i16(DEFAULT_SAMPLE_RATE, &bytes)?;
        first_audio_queue_ms = Some(play_start.elapsed().as_millis());
    }

    if save_full {
        tokio::fs::write("tts_full_download.pcm", &bytes)
            .await
            .context("Failed writing tts_full_download.pcm")?;
        println!("  saved: tts_full_download.pcm");
    }

    let result = BenchmarkResult {
        total_ms: recv_done.as_millis(),
        ttfb_ms: None,
        first_audio_queue_ms,
        bytes_received: bytes.len(),
    };

    println!(
        "  total receive: {} ms, bytes={}{}",
        result.total_ms,
        result.bytes_received,
        if playback { ", playback queued after full body" } else { "" }
    );
    println!();

    Ok(result)
}

async fn run_streaming(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    voice: &str,
    text: &str,
    playback: bool,
) -> Result<BenchmarkResult> {
    println!("[B] Streaming mode: start playback on first chunk");

    let start = Instant::now();
    let response = tts_request(client, api_key, model, voice, text).await?;

    let (_stream_guard, sink) = if playback {
        let (stream_guard, handle) = OutputStream::try_default().context("No audio output device")?;
        let sink = Sink::try_new(&handle).context("Failed creating rodio sink")?;
        (Some(stream_guard), Some(sink))
    } else {
        (None, None)
    };

    let mut ttfb_ms = None;
    let mut first_audio_queue_ms = None;
    let mut bytes_received = 0usize;
    let mut carry = Vec::<u8>::new();

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error while reading streaming chunk")?;

        if ttfb_ms.is_none() {
            ttfb_ms = Some(start.elapsed().as_millis());
        }

        bytes_received += chunk.len();

        if let Some(sink) = &sink {
            let samples = decode_pcm_chunk_i16_to_f32(&chunk, &mut carry);
            if !samples.is_empty() {
                if first_audio_queue_ms.is_none() {
                    first_audio_queue_ms = Some(start.elapsed().as_millis());
                }
                sink.append(SamplesBuffer::new(1, DEFAULT_SAMPLE_RATE, samples));
            }
        }
    }

    if let Some(sink) = &sink {
        if !carry.is_empty() {
            let samples = decode_pcm_chunk_i16_to_f32(&[], &mut carry);
            if !samples.is_empty() {
                if first_audio_queue_ms.is_none() {
                    first_audio_queue_ms = Some(start.elapsed().as_millis());
                }
                sink.append(SamplesBuffer::new(1, DEFAULT_SAMPLE_RATE, samples));
            }
        }
        sink.sleep_until_end();
    }

    let result = BenchmarkResult {
        total_ms: start.elapsed().as_millis(),
        ttfb_ms,
        first_audio_queue_ms,
        bytes_received,
    };

    println!(
        "  ttfb: {} ms, first_audio_queue: {} ms, total receive: {} ms, bytes={}",
        result.ttfb_ms.unwrap_or_default(),
        result.first_audio_queue_ms.unwrap_or_default(),
        result.total_ms,
        result.bytes_received
    );
    println!();

    Ok(result)
}

async fn tts_request(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    voice: &str,
    text: &str,
) -> Result<reqwest::Response> {
    let payload = json!({
        "model": model,
        "voice": voice,
        "input": text,
        "response_format": "pcm",
    });

    let response = client
        .post(OPENAI_API_URL)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .context("TTS request failed")?;

    if response.status() != StatusCode::OK {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("TTS API error {}: {}", status, body);
    }

    Ok(response)
}

fn decode_pcm_chunk_i16_to_f32(chunk: &[u8], carry: &mut Vec<u8>) -> Vec<f32> {
    if !carry.is_empty() {
        carry.extend_from_slice(chunk);
    } else {
        carry.extend_from_slice(chunk);
    }

    let even_len = carry.len() - (carry.len() % 2);
    let mut samples = Vec::with_capacity(even_len / 2);

    let mut i = 0usize;
    while i + 1 < even_len {
        let s = i16::from_le_bytes([carry[i], carry[i + 1]]);
        samples.push(s as f32 / i16::MAX as f32);
        i += 2;
    }

    let remainder = carry.split_off(even_len);
    carry.clear();
    carry.extend_from_slice(&remainder);

    samples
}

fn play_pcm_i16(sample_rate: u32, bytes: &[u8]) -> Result<()> {
    let (stream_guard, handle) = OutputStream::try_default().context("No audio output device")?;
    let sink = Sink::try_new(&handle).context("Failed creating rodio sink")?;

    let mut samples = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let s = i16::from_le_bytes([bytes[i], bytes[i + 1]]);
        samples.push(s as f32 / i16::MAX as f32);
        i += 2;
    }

    sink.append(SamplesBuffer::new(1, sample_rate, samples));
    sink.play();

    // Keep stream alive for queued playback
    let _keep_alive = stream_guard;
    Ok(())
}

fn print_summary(full: &BenchmarkResult, stream: &BenchmarkResult) {
    println!("=== Benchmark Summary ===");
    println!("| Metric | Full Download (A) | Streaming (B) |");
    println!("|---|---:|---:|");
    println!("| Total receive time | {} ms | {} ms |", full.total_ms, stream.total_ms);
    println!(
        "| Time to first byte (TTFB) | N/A | {} ms |",
        stream.ttfb_ms.unwrap_or_default()
    );
    let full_t1 = format_opt_ms(Some(full.total_ms));
    let stream_t2 = if stream.first_audio_queue_ms.is_some() {
        format_opt_ms(stream.first_audio_queue_ms)
    } else {
        "N/A (playback disabled)".to_string()
    };
    println!(
        "| Time to first audio queued | {} (after full body) | {} |",
        full_t1, stream_t2
    );

    if let Some(stream_first) = stream.first_audio_queue_ms {
        if full.total_ms > stream_first {
            let improvement = ((full.total_ms - stream_first) as f64 / full.total_ms as f64) * 100.0;
            println!(
                "\nFirst-audio latency improvement (B vs A): {:.1}%",
                improvement
            );
        }
    }

    println!("\nUse this in proposal as T1 (A) and T2 (B) evidence.");
}

fn print_aggregate_summary(records: &[RunRecord], playback: bool) {
    let full_t1: Vec<u128> = records.iter().map(|r| r.full.total_ms).collect();
    let stream_t2: Vec<u128> = records
        .iter()
        .map(|r| effective_stream_t2(&r.stream, playback))
        .collect();

    let full_stats = stats(&full_t1);
    let stream_stats = stats(&stream_t2);

    println!("\n=== Aggregate Summary ({} runs) ===", records.len());
    println!("| Metric | Full Download T1 | Streaming T2 |");
    println!("|---|---:|---:|");
    println!(
        "| Mean | {:.1} ms | {:.1} ms |",
        full_stats.mean_ms, stream_stats.mean_ms
    );
    println!(
        "| Median | {} ms | {} ms |",
        full_stats.median_ms, stream_stats.median_ms
    );
    println!("| P95 | {} ms | {} ms |", full_stats.p95_ms, stream_stats.p95_ms);
    println!("| Min | {} ms | {} ms |", full_stats.min_ms, stream_stats.min_ms);
    println!("| Max | {} ms | {} ms |", full_stats.max_ms, stream_stats.max_ms);

    if full_stats.mean_ms > stream_stats.mean_ms {
        let gain = ((full_stats.mean_ms - stream_stats.mean_ms) / full_stats.mean_ms) * 100.0;
        println!("\nMean first-audio latency improvement: {:.1}%", gain);
    }

    if !playback {
        println!(
            "\nNote: playback is disabled; streaming T2 falls back to TTFB for comparability."
        );
    }
}

fn effective_stream_t2(stream: &BenchmarkResult, playback: bool) -> u128 {
    if playback {
        stream.first_audio_queue_ms.unwrap_or(stream.total_ms)
    } else {
        stream.ttfb_ms.unwrap_or(stream.total_ms)
    }
}

fn stats(values: &[u128]) -> SummaryStats {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();

    let len = sorted.len();
    let sum: u128 = sorted.iter().copied().sum();
    let mean = sum as f64 / len as f64;
    let median = if len % 2 == 0 {
        let a = sorted[len / 2 - 1];
        let b = sorted[len / 2];
        (a + b) / 2
    } else {
        sorted[len / 2]
    };
    let p95_idx = ((len as f64 * 0.95).ceil() as usize).saturating_sub(1).min(len - 1);
    let p95 = sorted[p95_idx];

    SummaryStats {
        mean_ms: mean,
        median_ms: median,
        p95_ms: p95,
        min_ms: sorted[0],
        max_ms: sorted[len - 1],
    }
}

async fn write_csv(path: &str, records: &[RunRecord], playback: bool) -> Result<()> {
    let mut csv = String::from(
        "run,full_total_ms,full_t1_ms,stream_total_ms,stream_ttfb_ms,stream_first_audio_ms,stream_t2_effective_ms,bytes_full,bytes_stream\n",
    );

    for r in records {
        let stream_t2 = effective_stream_t2(&r.stream, playback);
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            r.run,
            r.full.total_ms,
            r.full.total_ms,
            r.stream.total_ms,
            r.stream.ttfb_ms.unwrap_or_default(),
            r.stream.first_audio_queue_ms.unwrap_or_default(),
            stream_t2,
            r.full.bytes_received,
            r.stream.bytes_received
        ));
    }

    tokio::fs::write(path, csv)
        .await
        .with_context(|| format!("Failed writing CSV to {path}"))?;

    Ok(())
}

fn format_opt_ms(v: Option<u128>) -> String {
    match v {
        Some(ms) => format!("{} ms", ms),
        None => "N/A".to_string(),
    }
}
