//! Text-to-Speech (TTS) Plugin Module
//!
//! Provides TTS capabilities using a generic TTS engine interface.
//! This module is designed to work with multiple TTS backends.
//!
//! Kokoro TTS is available behind the `kokoro` feature flag.
//! When enabled, initialization failures are treated as hard errors so
//! callers do not accidentally receive mock/silent synthesis.

// Model cache and download modules
pub mod cache;
pub mod model_downloader;
pub mod openai;

// Kokoro TTS wrapper (available with kokoro feature)
#[cfg(feature = "kokoro")]
pub mod kokoro_wrapper;

use crate::{AgentPlugin, PluginContext, PluginMetadata, PluginResult, PluginState, PluginType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

// ============================================================================
// Voice Information
// ============================================================================

/// Voice metadata information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceInfo {
    /// Voice identifier
    pub id: String,
    /// Human-readable voice name
    pub name: String,
    /// Language code (e.g., "en-US", "zh-CN")
    pub language: String,
}

impl VoiceInfo {
    pub fn new(id: &str, name: &str, language: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            language: language.to_string(),
        }
    }
}

// ============================================================================
// TTS Plugin Configuration
// ============================================================================

/// TTS plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSPluginConfig {
    /// Default voice to use for synthesis
    pub default_voice: String,
    /// Model version to use ("v1.0" or "v1.1" for Kokoro")
    pub model_version: String,
    /// Streaming chunk size in bytes
    pub streaming_chunk_size: usize,
    /// Hugging Face model URL (e.g., "hexgrad/Kokoro-82M")
    #[serde(default = "default_model_url")]
    pub model_url: String,
    /// Custom cache directory path (defaults to ~/.mofa/models/tts/)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<String>,
    /// Enable automatic model download if not found in cache
    #[serde(default = "default_auto_download")]
    pub auto_download: bool,
    /// Expected model checksum for validation (MD5 hex string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_checksum: Option<String>,
    /// Download timeout in seconds
    #[serde(default = "default_download_timeout")]
    pub download_timeout: u64,
}

fn default_model_url() -> String {
    "hexgrad/Kokoro-82M".to_string()
}

fn default_auto_download() -> bool {
    true
}

fn default_download_timeout() -> u64 {
    600 // 10 minutes default
}

impl Default for TTSPluginConfig {
    fn default() -> Self {
        Self {
            default_voice: "default".to_string(),
            model_version: "v1.1".to_string(),
            streaming_chunk_size: 4096,
            model_url: default_model_url(),
            cache_dir: None,
            auto_download: true,
            model_checksum: None,
            download_timeout: 600,
        }
    }
}

// ============================================================================
// TTS Engine Trait
// ============================================================================

/// Abstract TTS engine trait for extensibility
#[async_trait::async_trait]
pub trait TTSEngine: Send + Sync {
    /// Synthesize text to audio data
    async fn synthesize(&self, text: &str, voice: &str) -> PluginResult<Vec<u8>>;

    /// Synthesize with streaming callback for long texts
    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
        callback: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> PluginResult<()>;

    /// List available voices
    async fn list_voices(&self) -> PluginResult<Vec<VoiceInfo>>;

    /// Get engine name
    fn name(&self) -> &str;

    /// Get as Any for downcasting to engine-specific types
    ///
    /// This allows accessing engine-specific methods like stream_receiver()
    /// on KokoroTTS after downcasting.
    fn as_any(&self) -> &dyn std::any::Any;
}

// ============================================================================
// Mock TTS Engine (Placeholder)
// ============================================================================

/// A mock TTS engine for testing and development.
///
/// This engine generates placeholder WAV audio data. It's used when
/// a real TTS engine is not available or for testing purposes.
pub struct MockTTSEngine {
    config: TTSPluginConfig,
    voices: Vec<VoiceInfo>,
}

impl MockTTSEngine {
    /// Create a new mock TTS engine
    pub fn new(config: TTSPluginConfig) -> Self {
        let voices = vec![
            VoiceInfo::new("default", "Default Voice", "en-US"),
            VoiceInfo::new("af_heart", "Heart (Female)", "en-US"),
            VoiceInfo::new("am_michael", "Michael (Male)", "en-US"),
            VoiceInfo::new("bf_emma", "Emma (Female)", "en-US"),
            VoiceInfo::new("bm_george", "George (Male)", "en-US"),
            VoiceInfo::new("zh_female", "Chinese Female", "zh-CN"),
        ];

        Self { config, voices }
    }
}

#[async_trait::async_trait]
impl TTSEngine for MockTTSEngine {
    async fn synthesize(&self, text: &str, voice: &str) -> PluginResult<Vec<u8>> {
        debug!(
            "[MockTTS] Synthesizing text with voice '{}': {}",
            voice, text
        );

        // Generate a placeholder WAV file
        let sample_rate = 24000u32;
        let duration_sec = (text.len() as f32 / 15.0).ceil() as u32; // Rough estimate
        let num_samples = sample_rate * duration_sec;
        let data_size = num_samples * 2; // 16-bit samples

        // Generate WAV file header
        let mut wav_data = Vec::new();

        // RIFF header
        wav_data.extend_from_slice(b"RIFF");
        wav_data.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav_data.extend_from_slice(b"WAVE");

        // fmt chunk
        wav_data.extend_from_slice(b"fmt ");
        wav_data.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // audio format (PCM)
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // num channels (mono)
        wav_data.extend_from_slice(&sample_rate.to_le_bytes());
        wav_data.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav_data.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav_data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

        // data chunk
        wav_data.extend_from_slice(b"data");
        wav_data.extend_from_slice(&data_size.to_le_bytes());

        // Add silence (zeros) for audio data
        wav_data.resize(wav_data.len() + data_size as usize, 0);

        Ok(wav_data)
    }

    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
        callback: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> PluginResult<()> {
        debug!(
            "[MockTTS] Stream synthesizing text with voice '{}': {}",
            voice, text
        );

        // Split text into chunks for streaming
        let chunk_size = self.config.streaming_chunk_size;
        let chunks: Vec<&str> = text
            .as_bytes()
            .chunks(chunk_size)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect();

        for chunk in chunks {
            if chunk.is_empty() {
                continue;
            }
            let audio = self.synthesize(chunk, voice).await?;
            callback(audio);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn list_voices(&self) -> PluginResult<Vec<VoiceInfo>> {
        Ok(self.voices.clone())
    }

    fn name(&self) -> &str {
        "MockTTS"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Audio Playback Helper (Optional - Requires rodio feature)
// ============================================================================

#[cfg(feature = "rodio")]
use rodio::{Decoder, OutputStream, Sink};
#[cfg(feature = "rodio")]
use std::io::Cursor;

/// Audio playback configuration
#[derive(Debug, Clone)]
pub struct AudioPlaybackConfig {
    /// Whether to enable audio playback
    pub enabled: bool,
    /// Volume level (0.0 to 1.0)
    pub volume: f32,
}

impl Default for AudioPlaybackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.8,
        }
    }
}

/// Play audio data synchronously using rodio when feature is enabled
///
/// This function decodes and plays WAV audio data through the default audio output.
/// It blocks until playback completes.
///
/// # Arguments
///
/// * `audio_data` - WAV format audio data as bytes
///
/// # Returns
///
/// Returns `Ok(())` if playback succeeds, or an error if audio initialization or playback fails.
///
/// # Platform Support
///
/// - **macOS**: Works out of the box (Core Audio)
/// - **Linux**: Requires `libasound2-dev` (ALSA)
/// - **Windows**: Works out of the box (WasAPI)
#[cfg(feature = "rodio")]
pub fn play_audio(audio_data: Vec<u8>) -> PluginResult<()> {
    info!("Playing {} bytes of audio using rodio", audio_data.len());

    let cursor = Cursor::new(audio_data);
    let (_stream, stream_handle) = OutputStream::try_default().map_err(|e| {
        mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
            "Failed to get audio output: {}",
            e
        ))
    })?;
    let sink = Sink::try_new(&stream_handle).map_err(|e| {
        mofa_kernel::plugin::PluginError::ExecutionFailed(format!("Failed to create sink: {}", e))
    })?;

    let source = Decoder::new(cursor).map_err(|e| {
        mofa_kernel::plugin::PluginError::ExecutionFailed(format!("Failed to decode audio: {}", e))
    })?;
    sink.append(source);
    sink.sleep_until_end();

    Ok(())
}

/// Play audio data synchronously (fallback when rodio feature is not enabled)
///
/// When rodio is not enabled, this simulates playback with a delay.
/// Enable the rodio feature for actual audio playback.
#[cfg(not(feature = "rodio"))]
pub fn play_audio(audio_data: Vec<u8>) -> PluginResult<()> {
    debug!(
        "Playing {} bytes of audio (placeholder - rodio not enabled)",
        audio_data.len()
    );

    warn!(
        "Audio playback is simulated. Enable the 'rodio' feature in Cargo.toml \
         for actual audio playback support."
    );

    // Simulate playback delay based on audio size
    let delay_ms = std::cmp::min(500, audio_data.len() as u64 / 100);
    std::thread::sleep(std::time::Duration::from_millis(delay_ms));

    Ok(())
}

/// Play audio asynchronously
pub async fn play_audio_async(audio_data: Vec<u8>) -> PluginResult<()> {
    tokio::task::spawn_blocking(move || play_audio(audio_data))
        .await
        .map_err(|e| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Playback task failed: {}",
                e
            ))
        })?
}

// ============================================================================
// TTS Plugin
// ============================================================================

/// TTS Plugin implementing AgentPlugin
pub struct TTSPlugin {
    metadata: PluginMetadata,
    state: PluginState,
    config: TTSPluginConfig,
    engine: Option<Arc<dyn TTSEngine>>,
    synthesis_count: u64,
    total_chars_synthesized: u64,
    last_audio_data: Vec<u8>,
    /// Model cache manager
    model_cache: Option<cache::ModelCache>,
    /// Hugging Face download client
    hf_client: Option<model_downloader::HFHubClient>,
}

impl TTSPlugin {
    /// Create a new TTS plugin
    pub fn new(plugin_id: &str) -> Self {
        let metadata = PluginMetadata::new(plugin_id, "TTS Plugin", PluginType::Tool)
            .with_description("Text-to-Speech plugin with support for multiple TTS engines")
            .with_capability("text_to_speech")
            .with_capability("audio_synthesis")
            .with_capability("streaming_synthesis")
            .with_capability("model_download");

        Self {
            metadata,
            state: PluginState::Unloaded,
            config: TTSPluginConfig::default(),
            engine: None,
            synthesis_count: 0,
            total_chars_synthesized: 0,
            last_audio_data: Vec::new(),
            model_cache: None,
            hf_client: None,
        }
    }

    /// Create a TTS plugin with engine and voice (便捷方法)
    ///
    /// # 参数
    ///
    /// - `plugin_id`: 插件ID
    /// - `engine`: TTS 引擎
    /// - `default_voice`: 默认音色，如 `"zf_090"`，默认为 `"default"`
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// use mofa_plugins::TTSPlugin;
    ///
    /// // 使用默认音色
    /// let plugin = TTSPlugin::with_engine("tts", kokoro_engine, None);
    ///
    /// // 指定音色
    /// let plugin = TTSPlugin::with_engine("tts", kokoro_engine, Some("zf_090"));
    /// ```
    pub fn with_engine<E: TTSEngine + 'static>(
        plugin_id: &str,
        engine: E,
        default_voice: Option<&str>,
    ) -> Self {
        let mut plugin = Self::new(plugin_id);
        plugin.engine = Some(Arc::new(engine));
        if let Some(voice) = default_voice {
            plugin.config.default_voice = voice.to_string();
        }
        plugin
    }

    /// Set the plugin configuration
    pub fn with_config(mut self, config: TTSPluginConfig) -> Self {
        self.config = config;
        self
    }

    /// Set a custom TTS engine (链式调用版本，用于已有实例)
    pub fn with_engine_ref<E: TTSEngine + 'static>(mut self, engine: E) -> Self {
        self.engine = Some(Arc::new(engine));
        self
    }

    /// Set the default voice
    pub fn with_voice(mut self, voice: &str) -> Self {
        self.config.default_voice = voice.to_string();
        self
    }

    /// Get the TTS engine
    pub fn engine(&self) -> Option<&Arc<dyn TTSEngine>> {
        self.engine.as_ref()
    }

    /// Synthesize text to audio and play it
    pub async fn synthesize_and_play(&mut self, text: &str) -> PluginResult<()> {
        let engine = self.engine.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "TTS engine not initialized".to_string(),
            )
        })?;

        self.synthesis_count += 1;
        self.total_chars_synthesized += text.len() as u64;

        let voice = self.config.default_voice.as_str();
        let audio = engine.synthesize(text, voice).await?;
        play_audio_async(audio).await?;
        Ok(())
    }

    /// Synthesize text to audio data (no playback)
    pub async fn synthesize_to_audio(&mut self, text: &str) -> PluginResult<Vec<u8>> {
        let engine = self.engine.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "TTS engine not initialized".to_string(),
            )
        })?;

        self.synthesis_count += 1;
        self.total_chars_synthesized += text.len() as u64;

        let voice = self.config.default_voice.as_str();
        engine.synthesize(text, voice).await
    }

    /// Stream synthesize text with callback
    pub async fn synthesize_streaming(
        &mut self,
        text: &str,
        callback: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> PluginResult<()> {
        let engine = self.engine.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "TTS engine not initialized".to_string(),
            )
        })?;

        self.synthesis_count += 1;
        self.total_chars_synthesized += text.len() as u64;

        let voice = self.config.default_voice.as_str();
        engine.synthesize_stream(text, voice, callback).await
    }

    /// Stream synthesize text with f32 callback (native format)
    ///
    /// This method is more efficient for KokoroTTS as it uses the native f32 format
    /// without the overhead of f32 -> i16 -> u8 conversion.
    ///
    /// # Arguments
    /// - `text`: The text to synthesize
    /// - `callback`: Function to call with each audio chunk (`Vec<f32>`)
    ///
    /// # Example
    /// ```rust,ignore
    /// plugin.synthesize_streaming_f32("Hello", Box::new(|audio_f32| {
    ///     // audio_f32 is `Vec<f32>` with values in [-1.0, 1.0]
    ///     sink.append(SamplesBuffer::new(1, 24000, audio_f32));
    /// })).await?;
    /// ```
    pub async fn synthesize_streaming_f32(
        &mut self,
        text: &str,
        callback: Box<dyn Fn(Vec<f32>) + Send + Sync>,
    ) -> PluginResult<()> {
        let engine = self.engine.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "TTS engine not initialized".to_string(),
            )
        })?;

        self.synthesis_count += 1;
        self.total_chars_synthesized += text.len() as u64;

        let voice = self.config.default_voice.as_str();

        #[cfg(feature = "kokoro")]
        {
            // Try to downcast to KokoroTTS for native f32 streaming
            if let Some(kokoro) = engine.as_any().downcast_ref::<kokoro_wrapper::KokoroTTS>() {
                return kokoro
                    .synthesize_stream_f32(text, voice, callback)
                    .await
                    .map_err(|e| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                            "F32 streaming failed: {}",
                            e
                        ))
                    });
            }
        }

        // Fallback: synthesize to bytes first, then convert to f32
        // We can't directly pass callback to synthesize_stream because of ownership issues
        let audio_bytes = engine.synthesize(text, voice).await?;

        // Convert bytes to f32 and call callback
        let audio_i16: Vec<i16> = audio_bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let audio_f32: Vec<f32> = audio_i16
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();
        callback(audio_f32);

        Ok(())
    }

    /// List available voices
    pub async fn list_voices(&self) -> PluginResult<Vec<VoiceInfo>> {
        let engine = self.engine.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "TTS engine not initialized".to_string(),
            )
        })?;
        engine.list_voices().await
    }

    /// Set the default voice
    pub fn set_default_voice(&mut self, voice: &str) {
        self.config.default_voice = voice.to_string();
    }

    /// Get synthesis statistics
    pub fn stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        stats.insert(
            "synthesis_count".to_string(),
            serde_json::json!(self.synthesis_count),
        );
        stats.insert(
            "total_chars".to_string(),
            serde_json::json!(self.total_chars_synthesized),
        );
        stats.insert(
            "default_voice".to_string(),
            serde_json::json!(self.config.default_voice),
        );
        stats.insert(
            "model_version".to_string(),
            serde_json::json!(self.config.model_version),
        );
        stats.insert(
            "model_url".to_string(),
            serde_json::json!(self.config.model_url),
        );
        stats.insert(
            "auto_download".to_string(),
            serde_json::json!(self.config.auto_download),
        );
        // Add cache info if available
        if let Some(cache) = &self.model_cache
            && let Some(cache_dir) = cache.cache_dir().to_str()
        {
            stats.insert("cache_dir".to_string(), serde_json::json!(cache_dir));
        }
        if let Some(engine) = &self.engine {
            stats.insert("engine".to_string(), serde_json::json!(engine.name()));
        }
        stats
    }

    /// Get the last synthesized audio data
    pub fn last_audio(&self) -> Vec<u8> {
        self.last_audio_data.clone()
    }
}

#[async_trait::async_trait]
impl AgentPlugin for TTSPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn state(&self) -> PluginState {
        self.state.clone()
    }

    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Loading;
        info!("Loading TTS plugin: {}", self.metadata.id);

        // Load configuration from context
        if let Some(default_voice) = ctx.config.get_string("default_voice") {
            self.config.default_voice = default_voice;
        }
        if let Some(model_version) = ctx.config.get_string("model_version") {
            self.config.model_version = model_version;
        }
        if let Some(model_url) = ctx.config.get_string("model_url") {
            self.config.model_url = model_url;
        }
        if let Some(cache_dir) = ctx.config.get_string("cache_dir") {
            self.config.cache_dir = Some(cache_dir);
        }
        if let Some(auto_download) = ctx.config.get_bool("auto_download") {
            self.config.auto_download = auto_download;
        }
        if let Some(checksum) = ctx.config.get_string("model_checksum") {
            self.config.model_checksum = Some(checksum);
        }

        // Initialize model cache
        let cache_dir = self.config.cache_dir.as_ref().map(std::path::PathBuf::from);
        self.model_cache = Some(cache::ModelCache::new(cache_dir).map_err(|e| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Failed to initialize model cache: {}",
                e
            ))
        })?);

        // Initialize Hugging Face client
        self.hf_client = Some(model_downloader::HFHubClient::new());

        self.state = PluginState::Loaded;
        Ok(())
    }

    async fn init_plugin(&mut self) -> PluginResult<()> {
        info!("Initializing TTS plugin: {}", self.metadata.id);

        // Check if we need to download the model
        let cache = self.model_cache.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "Model cache not initialized".to_string(),
            )
        })?;

        let hf_client = self.hf_client.as_ref().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "HF client not initialized".to_string(),
            )
        })?;

        // Check if model exists in cache
        let model_exists = cache.exists(&self.config.model_url).await;

        if !model_exists && self.config.auto_download {
            info!(
                "Model not found in cache, initiating download: {}",
                self.config.model_url
            );

            let download_config = model_downloader::DownloadConfig {
                model_id: self.config.model_url.clone(),
                filename: "kokoro-v0_19.onnx".to_string(),
                checksum: self.config.model_checksum.clone(),
                timeout_secs: self.config.download_timeout,
                max_retries: 3,
                progress_callback: Some(Box::new(|downloaded, total| {
                    let progress = if total > 0 {
                        format!("{:.1}%", (downloaded as f64 / total as f64) * 100.0)
                    } else {
                        format!("{} bytes", downloaded)
                    };
                    info!("Download progress: {}", progress);
                })),
            };

            hf_client
                .download_model(download_config, cache)
                .await
                .map_err(|e| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                        "Failed to download model: {}",
                        e
                    ))
                })?;
        } else if !model_exists {
            // Auto-download disabled, fail with clear error
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Model '{}' not found in cache and auto_download is disabled. \
                Please enable auto_download or manually download the model to: {:?}",
                self.config.model_url,
                cache.model_path(&self.config.model_url)
            )));
        }

        // Validate cached model
        if let Some(expected_checksum) = &self.config.model_checksum
            && !cache
                .validate(&self.config.model_url, Some(expected_checksum))
                .await
                .map_err(|e| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                        "Failed to validate model: {}",
                        e
                    ))
                })?
        {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(
                "Model validation failed. The cached model may be corrupted. \
                    Try deleting the cache and re-downloading."
                    .into(),
            ));
        }

        // Initialize engine with downloaded/cached model
        if self.engine.is_none() {
            #[cfg(feature = "kokoro")]
            {
                // Try to initialize Kokoro engine
                let model_path = cache.model_path(&self.config.model_url);
                let voice_path_buf = model_path
                    .parent()
                    .map(|p| p.join("voices-v1.1-zh.bin"))
                    .unwrap_or_else(|| std::path::PathBuf::from("voices-v1.1-zh.bin"));
                let voice_path = voice_path_buf.to_str().unwrap_or("voices-v1.1-zh.bin");

                let model_path_str = model_path.to_str().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Invalid model path".to_string(),
                    )
                })?;

                info!(
                    "Initializing Kokoro TTS engine with model: {}, voices: {}",
                    model_path_str, voice_path
                );

                match kokoro_wrapper::KokoroTTS::new(model_path_str, voice_path).await {
                    Ok(engine) => {
                        self.engine = Some(Arc::new(engine));
                        info!("Kokoro TTS engine initialized successfully");
                    }
                    Err(e) => {
                        return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                            "Failed to initialize Kokoro engine (model: {}, voices: {}): {}",
                            model_path_str, voice_path, e
                        )));
                    }
                }
            }

            #[cfg(not(feature = "kokoro"))]
            {
                // Fallback to mock engine when feature not enabled
                warn!("Kokoro feature not enabled, using mock engine");
                let engine = MockTTSEngine::new(self.config.clone());
                self.engine = Some(Arc::new(engine));
            }
        }

        Ok(())
    }

    async fn start(&mut self) -> PluginResult<()> {
        self.state = PluginState::Running;
        info!("TTS plugin {} started", self.metadata.id);
        Ok(())
    }

    async fn stop(&mut self) -> PluginResult<()> {
        self.state = PluginState::Loaded;
        info!("TTS plugin {} stopped", self.metadata.id);
        Ok(())
    }

    async fn unload(&mut self) -> PluginResult<()> {
        self.engine = None;
        self.state = PluginState::Unloaded;
        info!("TTS plugin {} unloaded", self.metadata.id);
        Ok(())
    }

    async fn execute(&mut self, input: String) -> PluginResult<String> {
        // Parse input as JSON command
        let command: TTSCommand = serde_json::from_str(&input).map_err(|e| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Invalid TTS command format: {}",
                e
            ))
        })?;

        match command.action.as_str() {
            "speak" | "synthesize" => {
                let text = command.text.ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Missing 'text' parameter".to_string(),
                    )
                })?;

                if command.play.unwrap_or(true) {
                    self.synthesize_and_play(&text).await?;
                    Ok(format!("Played: {}", text))
                } else {
                    let audio = self.synthesize_to_audio(&text).await?;
                    // Store the audio data for later retrieval
                    self.last_audio_data = audio.clone();
                    Ok(format!("Generated {} bytes of audio", audio.len()))
                }
            }
            "list_voices" => {
                let voices = self.list_voices().await?;
                let json = serde_json::to_string(&voices).map_err(|e| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(e.to_string())
                })?;
                Ok(json)
            }
            "set_voice" => {
                let voice = command.voice.ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Missing 'voice' parameter".to_string(),
                    )
                })?;
                self.set_default_voice(&voice);
                Ok(format!("Default voice set to: {}", voice))
            }
            "stats" => {
                let stats = self.stats();
                let json = serde_json::to_string(&stats).map_err(|e| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(e.to_string())
                })?;
                Ok(json)
            }
            _ => Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Unknown action: {}",
                command.action
            ))),
        }
    }

    fn stats(&self) -> HashMap<String, serde_json::Value> {
        self.stats()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ============================================================================
// TTS Command Types
// ============================================================================

/// TTS command structure for execute()
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSCommand {
    /// Action to perform: "speak", "list_voices", "set_voice", "stats"
    pub action: String,
    /// Text to synthesize (for "speak" action)
    pub text: Option<String>,
    /// Voice to use (for "speak" or "set_voice" action)
    pub voice: Option<String>,
    /// Whether to play audio (for "speak" action)
    pub play: Option<bool>,
}

// ============================================================================
// Tool Executor for TTS
// ============================================================================

use crate::ToolDefinition;
use crate::ToolExecutor;

/// Text-to-Speech tool executor
pub struct TextToSpeechTool {
    plugin_id: String,
    definition: ToolDefinition,
}

impl TextToSpeechTool {
    pub fn new(plugin_id: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            definition: ToolDefinition {
                name: "text_to_speech".to_string(),
                description: "Convert text to speech using the TTS plugin engine".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The text to synthesize to speech"
                        },
                        "voice": {
                            "type": "string",
                            "description": "Voice ID to use (optional, uses default if not specified)",
                            "default": "default"
                        },
                        "play": {
                            "type": "boolean",
                            "description": "Whether to play the audio (true) or return audio data (false)",
                            "default": true
                        }
                    },
                    "required": ["text"]
                }),
                requires_confirmation: false,
            },
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for TextToSpeechTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        let text = arguments
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                mofa_kernel::plugin::PluginError::ExecutionFailed(
                    "Missing 'text' parameter".to_string(),
                )
            })?;

        let voice = arguments.get("voice").and_then(|v| v.as_str());
        let play = arguments
            .get("play")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let command = if let Some(voice) = voice {
            TTSCommand {
                action: "speak".to_string(),
                text: Some(text.to_string()),
                voice: Some(voice.to_string()),
                play: Some(play),
            }
        } else {
            TTSCommand {
                action: "speak".to_string(),
                text: Some(text.to_string()),
                voice: None,
                play: Some(play),
            }
        };

        let input = serde_json::to_string(&command)
            .map_err(|e| mofa_kernel::plugin::PluginError::ExecutionFailed(e.to_string()))?;
        Ok(serde_json::json!({
            "success": true,
            "message": format!("TTS command prepared for: {}", text),
            "command": input
        }))
    }

    fn validate(&self, arguments: &serde_json::Value) -> PluginResult<()> {
        if !arguments.is_object() {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(
                "Arguments must be an object".to_string(),
            ));
        }
        if arguments.get("text").and_then(|v| v.as_str()).is_none() {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(
                "Missing required parameter: text".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn prepare_cached_model(plugin: &mut TTSPlugin, model_id: &str, cache_dir: &std::path::Path) {
        plugin.config.model_url = model_id.to_string();
        plugin.config.cache_dir = Some(cache_dir.to_string_lossy().to_string());
        plugin.config.auto_download = false;
    }

    #[tokio::test]
    async fn test_mock_tts_engine_creation() {
        let config = TTSPluginConfig::default();
        let engine = MockTTSEngine::new(config);
        assert_eq!(engine.name(), "MockTTS");
    }

    #[tokio::test]
    async fn test_mock_tts_list_voices() {
        let config = TTSPluginConfig::default();
        let engine = MockTTSEngine::new(config);
        let voices = engine.list_voices().await.unwrap();

        assert!(!voices.is_empty());
        assert!(voices.iter().any(|v| v.id == "default"));
    }

    #[tokio::test]
    async fn test_tts_plugin_creation() {
        let plugin = TTSPlugin::new("test_tts");
        assert_eq!(plugin.plugin_id(), "test_tts");
        assert_eq!(plugin.state(), PluginState::Unloaded);
    }

    #[tokio::test]
    async fn test_tts_plugin_lifecycle() {
        let mut plugin = TTSPlugin::new("test_tts");
        let ctx = PluginContext::new("test_agent");

        plugin.load(&ctx).await.unwrap();
        assert_eq!(plugin.state(), PluginState::Loaded);

        // Use mock engine to avoid model download in tests
        let mock_engine = MockTTSEngine::new(TTSPluginConfig::default());
        plugin.engine = Some(Arc::new(mock_engine));

        plugin.start().await.unwrap();
        assert_eq!(plugin.state(), PluginState::Running);

        plugin.stop().await.unwrap();
        assert_eq!(plugin.state(), PluginState::Loaded);

        plugin.unload().await.unwrap();
        assert_eq!(plugin.state(), PluginState::Unloaded);
    }

    #[tokio::test]
    async fn test_tts_execute_speak_command() {
        let mut plugin = TTSPlugin::new("test_tts");
        let ctx = PluginContext::new("test_agent");

        plugin.load(&ctx).await.unwrap();

        // Use mock engine to avoid model download in tests
        let mock_engine = MockTTSEngine::new(TTSPluginConfig::default());
        plugin.engine = Some(Arc::new(mock_engine));

        plugin.start().await.unwrap();

        let command = TTSCommand {
            action: "speak".to_string(),
            text: Some("Hello, world!".to_string()),
            voice: None,
            play: Some(false), // Don't actually play audio in tests
        };

        let input = serde_json::to_string(&command).unwrap();
        let result = plugin.execute(input).await;

        // Should succeed with placeholder implementation
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tts_execute_list_voices() {
        let mut plugin = TTSPlugin::new("test_tts");
        let ctx = PluginContext::new("test_agent");

        plugin.load(&ctx).await.unwrap();

        // Use mock engine to avoid model download in tests
        let mock_engine = MockTTSEngine::new(TTSPluginConfig::default());
        plugin.engine = Some(Arc::new(mock_engine));

        plugin.start().await.unwrap();

        let command = TTSCommand {
            action: "list_voices".to_string(),
            text: None,
            voice: None,
            play: None,
        };

        let input = serde_json::to_string(&command).unwrap();
        let result = plugin.execute(input).await.unwrap();

        let voices: Vec<VoiceInfo> = serde_json::from_str(&result).unwrap();
        assert!(!voices.is_empty());
    }

    #[tokio::test]
    async fn test_tts_stats() {
        let plugin = TTSPlugin::new("test_tts");
        let stats = plugin.stats();

        assert_eq!(stats.get("synthesis_count"), Some(&serde_json::json!(0)));
        assert_eq!(stats.get("total_chars"), Some(&serde_json::json!(0)));
    }

    #[test]
    fn test_voice_info_creation() {
        let voice = VoiceInfo::new("test", "Test Voice", "en-US");
        assert_eq!(voice.id, "test");
        assert_eq!(voice.name, "Test Voice");
        assert_eq!(voice.language, "en-US");
    }

    #[test]
    fn test_tts_command_serialization() {
        let command = TTSCommand {
            action: "speak".to_string(),
            text: Some("Hello".to_string()),
            voice: Some("default".to_string()),
            play: Some(true),
        };

        let json = serde_json::to_string(&command).unwrap();
        let parsed: TTSCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.action, "speak");
        assert_eq!(parsed.text, Some("Hello".to_string()));
        assert_eq!(parsed.voice, Some("default".to_string()));
        assert_eq!(parsed.play, Some(true));
    }

    #[cfg(feature = "kokoro")]
    #[tokio::test]
    async fn test_init_plugin_kokoro_mode_fails_instead_of_mock_fallback() {
        let mut plugin = TTSPlugin::new("test_tts");
        let cache_dir = TempDir::new().unwrap();
        prepare_cached_model(&mut plugin, "test/kokoro.onnx", cache_dir.path());

        let ctx = PluginContext::new("test_agent");
        plugin.load(&ctx).await.unwrap();

        let cached_model_path = plugin
            .model_cache
            .as_ref()
            .unwrap()
            .model_path(&plugin.config.model_url);
        fs::write(&cached_model_path, b"fake-model-content").unwrap();

        let result = plugin.init_plugin().await;
        assert!(result.is_err());
    }

    #[cfg(not(feature = "kokoro"))]
    #[tokio::test]
    async fn test_init_plugin_without_kokoro_uses_mock_engine() {
        let mut plugin = TTSPlugin::new("test_tts");
        let cache_dir = TempDir::new().unwrap();
        prepare_cached_model(&mut plugin, "test/kokoro.onnx", cache_dir.path());

        let ctx = PluginContext::new("test_agent");
        plugin.load(&ctx).await.unwrap();

        let cached_model_path = plugin
            .model_cache
            .as_ref()
            .unwrap()
            .model_path(&plugin.config.model_url);
        fs::write(&cached_model_path, b"fake-model-content").unwrap();

        plugin.init_plugin().await.unwrap();
        assert_eq!(plugin.engine.as_ref().unwrap().name(), "MockTTS");
    }

    /// Regression test for Issue #534 (follow-up to #448):
    /// TTSPlugin::stop() must transition to Loaded, not Paused.
    /// stop() = proper shutdown (resources intact, not running)
    /// pause() = temporary suspension (can be resumed)
    #[tokio::test]
    async fn test_tts_plugin_stop_sets_loaded_state() {
        let mut plugin = TTSPlugin::new("test_tts");
        let ctx = PluginContext::new("test_agent");

        plugin.load(&ctx).await.unwrap();

        // Use mock engine to avoid model download in tests
        let mock_engine = MockTTSEngine::new(TTSPluginConfig::default());
        plugin.engine = Some(Arc::new(mock_engine));

        plugin.start().await.unwrap();
        assert_eq!(plugin.state(), PluginState::Running);

        plugin.stop().await.unwrap();
        assert_eq!(
            plugin.state(),
            PluginState::Loaded,
            "stop() must set state to Loaded, not Paused (Issue #534)"
        );
    }
}
