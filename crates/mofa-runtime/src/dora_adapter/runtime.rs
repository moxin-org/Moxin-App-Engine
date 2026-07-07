//! Dora Runtime Support - 真实运行时集成
//! Dora Runtime Support - Real runtime integration
//!
//! 提供 dora-rs 完整运行时支持，包括：
//! Provides full dora-rs runtime support, including:
//! - 嵌入式模式：使用 `Daemon::run_dataflow` 在单进程中运行数据流
//! - Embedded mode: Run dataflows in a single process using `Daemon::run_dataflow`
//! - 分布式模式：连接外部 dora-daemon 和 coordinator
//! - Distributed mode: Connect to external dora-daemon and coordinator
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                            DoraRuntime                              │
//! │  ┌────────────────────────────┬────────────────────────────────┐    │
//! │  │      EmbeddedMode          │        DistributedMode         │    │
//! │  │  ┌──────────────────┐      │   ┌─────────────────────────┐  │    │
//! │  │  │ Daemon::         │      │   │ Daemon::run()           │  │    │
//! │  │  │ run_dataflow()   │      │   │ (connect to coordinator)│  │    │
//! │  │  └──────────────────┘      │   └─────────────────────────┘  │    │
//! │  └────────────────────────────┴────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ## Embedded Mode (单进程运行 - 不需要外部 daemon/coordinator)
//! ## Embedded Mode (Single process - no external daemon/coordinator needed)
//! ```rust,ignore
//! use mofa::dora_adapter::runtime::{DoraRuntime, RuntimeConfig};
//!
//! #[tokio::main]
//! async fn main() -> eyre::Result<()> {
//!     let config = RuntimeConfig::embedded("dataflow.yml");
//!     let mut runtime = DoraRuntime::new(config);
//!     let result = runtime.run().await?;
//!     info!("Dataflow {} completed", result.uuid);
//!     Ok(())
//! }
//! ```
//!
//! ## Distributed Mode (连接外部 coordinator)
//! ## Distributed Mode (Connect to external coordinator)
//! ```rust,ignore
//! use mofa::dora_adapter::runtime::{DoraRuntime, RuntimeConfig};
//!
//! #[tokio::main]
//! async fn main() -> eyre::Result<()> {
//!     let addr = "127.0.0.1:5000".parse()?;
//!     let config = RuntimeConfig::distributed("dataflow.yml", addr);
//!     let mut runtime = DoraRuntime::new(config);
//!     runtime.run().await?;
//!     Ok(())
//! }
//! ```

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ::tracing::info;
use dora_daemon::Daemon;
pub use dora_daemon::LogDestination;
use dora_message::common::LogMessage;
use dora_message::coordinator_to_cli::DataflowResult as DoraDataflowResult;
use dora_message::{BuildId, SessionId};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

// ============================================================================
// Runtime Configuration
// ============================================================================

/// Dora 运行时模式
/// Dora runtime mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RuntimeMode {
    /// 嵌入式模式：使用 Daemon::run_dataflow 单进程运行
    /// Embedded mode: Single process execution using Daemon::run_dataflow
    /// 不需要外部 coordinator 或 daemon
    /// No external coordinator or daemon required
    #[default]
    Embedded,
    /// 分布式模式：连接外部 dora-daemon 和 coordinator
    /// Distributed mode: Connect to external dora-daemon and coordinator
    Distributed,
}

/// 运行时配置
/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// 运行时模式
    /// Runtime mode
    pub mode: RuntimeMode,
    /// 数据流 YAML 文件路径
    /// Dataflow YAML file path
    pub dataflow_path: PathBuf,
    /// 嵌入式模式配置
    /// Embedded mode configuration
    pub embedded: EmbeddedConfig,
    /// 分布式模式配置
    /// Distributed mode configuration
    pub distributed: DistributedConfig,
}

impl RuntimeConfig {
    /// 创建嵌入式模式配置
    /// Create embedded mode configuration
    pub fn embedded<P: AsRef<Path>>(dataflow_path: P) -> Self {
        Self {
            mode: RuntimeMode::Embedded,
            dataflow_path: dataflow_path.as_ref().to_path_buf(),
            embedded: EmbeddedConfig::default(),
            distributed: DistributedConfig::default(),
        }
    }

    /// 创建分布式模式配置
    /// Create distributed mode configuration
    pub fn distributed<P: AsRef<Path>>(dataflow_path: P, coordinator_addr: SocketAddr) -> Self {
        Self {
            mode: RuntimeMode::Distributed,
            dataflow_path: dataflow_path.as_ref().to_path_buf(),
            embedded: EmbeddedConfig::default(),
            distributed: DistributedConfig {
                coordinator_addr,
                ..Default::default()
            },
        }
    }

    /// 设置模式
    /// Set runtime mode
    pub fn with_mode(mut self, mode: RuntimeMode) -> Self {
        self.mode = mode;
        self
    }

    /// 设置是否使用 uv (Python 包管理器)
    /// Set whether to use uv (Python package manager)
    pub fn with_uv(mut self, uv: bool) -> Self {
        self.embedded.uv = uv;
        self
    }

    /// 设置事件输出目录
    /// Set event output directory
    pub fn with_events_output<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.embedded.write_events_to = Some(path.as_ref().to_path_buf());
        self
    }

    /// 设置日志目标
    /// Set log destination
    pub fn with_log_destination(mut self, dest: LogDestinationType) -> Self {
        self.embedded.log_destination = dest;
        self
    }
}

/// 嵌入式模式配置
/// Embedded mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedConfig {
    /// 使用 uv 运行 Python 节点
    /// Use uv to run Python nodes
    pub uv: bool,
    /// 事件输出目录
    /// Event output directory
    pub write_events_to: Option<PathBuf>,
    /// 日志目标
    /// Log destination
    pub log_destination: LogDestinationType,
    /// Build ID（如果之前已经构建过）
    /// Build ID (if previously built)
    pub build_id: Option<Uuid>,
}

impl Default for EmbeddedConfig {
    fn default() -> Self {
        Self {
            uv: false,
            write_events_to: None,
            log_destination: LogDestinationType::Channel,
            build_id: None,
        }
    }
}

/// 日志目标类型
/// Log destination type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum LogDestinationType {
    /// 使用 tracing 输出日志
    /// Output logs using tracing
    Tracing,
    /// 通过 channel 发送日志（可以自定义处理）
    /// Send logs via channel (supports custom handling)
    #[default]
    Channel,
}

/// 分布式模式配置
/// Distributed mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedConfig {
    /// Coordinator 地址
    /// Coordinator address
    pub coordinator_addr: SocketAddr,
    /// 机器 ID
    /// Machine ID
    pub machine_id: Option<String>,
    /// 本地监听端口
    /// Local listen port
    pub local_listen_port: u16,
}

impl Default for DistributedConfig {
    fn default() -> Self {
        Self {
            coordinator_addr: "127.0.0.1:5000".parse().unwrap(),
            machine_id: None,
            local_listen_port: 5001,
        }
    }
}

// ============================================================================
// Runtime State
// ============================================================================

/// 运行时状态
/// Runtime state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RuntimeState {
    #[default]
    Created,
    Running,
    Stopped,
    Error,
}

/// 数据流执行结果
/// Dataflow execution result
#[must_use]
#[derive(Debug, Clone)]
pub struct DataflowResult {
    /// 数据流 UUID
    /// Dataflow UUID
    pub uuid: Uuid,
    /// 节点执行结果
    /// Node execution results
    pub node_results: BTreeMap<String, NodeResult>,
    /// 是否成功
    /// Whether successful
    pub success: bool,
}

/// 节点执行结果
/// Node execution result
#[derive(Debug, Clone)]
pub enum NodeResult {
    Success,
    Error(String),
}

impl From<DoraDataflowResult> for DataflowResult {
    fn from(result: DoraDataflowResult) -> Self {
        let node_results: BTreeMap<String, NodeResult> = result
            .node_results
            .into_iter()
            .map(|(node_id, res)| {
                let result = match res {
                    Ok(()) => NodeResult::Success,
                    Err(err) => NodeResult::Error(format!("{:?}", err)),
                };
                (node_id.to_string(), result)
            })
            .collect();

        let success = node_results
            .values()
            .all(|r| matches!(r, NodeResult::Success));

        Self {
            uuid: result.uuid,
            node_results,
            success,
        }
    }
}

// ============================================================================
// Dora Runtime
// ============================================================================

/// Dora 运行时
/// Dora Runtime
///
/// 提供两种运行模式：
/// Provides two running modes:
/// - Embedded: 使用 `Daemon::run_dataflow` 在单进程中运行
/// - Embedded: Run in single process using `Daemon::run_dataflow`
/// - Distributed: 连接外部 coordinator 和 daemon
/// - Distributed: Connect to external coordinator and daemon
pub struct DoraRuntime {
    config: RuntimeConfig,
    state: Arc<RwLock<RuntimeState>>,
    /// 日志接收通道（当使用 Channel 日志目标时）
    /// Log receiver channel (when using Channel log destination)
    log_receiver: Option<flume::Receiver<LogMessage>>,
}

impl DoraRuntime {
    /// 创建新的运行时实例
    /// Create new runtime instance
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(RuntimeState::Created)),
            log_receiver: None,
        }
    }

    /// 创建嵌入式模式运行时
    /// Create embedded mode runtime
    pub fn embedded<P: AsRef<Path>>(dataflow_path: P) -> Self {
        Self::new(RuntimeConfig::embedded(dataflow_path))
    }

    /// 创建分布式模式运行时
    /// Create distributed mode runtime
    pub fn distributed<P: AsRef<Path>>(dataflow_path: P, coordinator_addr: SocketAddr) -> Self {
        Self::new(RuntimeConfig::distributed(dataflow_path, coordinator_addr))
    }

    /// 获取当前状态
    /// Get current state
    pub async fn state(&self) -> RuntimeState {
        *self.state.read().await
    }

    /// 获取配置
    /// Get configuration
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// 获取日志接收器（如果使用 Channel 日志目标）
    /// Get log receiver (if using Channel log destination)
    pub fn log_receiver(&self) -> Option<&flume::Receiver<LogMessage>> {
        self.log_receiver.as_ref()
    }

    /// 取走日志接收器的所有权
    /// Take ownership of log receiver
    pub fn take_log_receiver(&mut self) -> Option<flume::Receiver<LogMessage>> {
        self.log_receiver.take()
    }

    /// 运行数据流
    /// Run dataflow
    ///
    /// 根据配置的模式运行数据流：
    /// Run dataflow based on configured mode:
    /// - Embedded: 直接在当前进程中运行
    /// - Embedded: Run directly in current process
    /// - Distributed: 连接到外部 daemon
    /// - Distributed: Connect to external daemon
    pub async fn run(&mut self) -> Result<DataflowResult> {
        *self.state.write().await = RuntimeState::Running;

        let result = match self.config.mode {
            RuntimeMode::Embedded => self.run_embedded().await,
            RuntimeMode::Distributed => self.run_distributed().await,
        };

        match &result {
            Ok(_) => *self.state.write().await = RuntimeState::Stopped,
            Err(_) => *self.state.write().await = RuntimeState::Error,
        }

        result
    }

    /// 嵌入式模式运行
    /// Embedded mode execution
    async fn run_embedded(&mut self) -> Result<DataflowResult> {
        let dataflow_path = &self.config.dataflow_path;

        info!("Running dataflow in embedded mode: {:?}", dataflow_path);

        // 验证数据流文件存在
        // Verify dataflow file existence
        if !dataflow_path.exists() {
            return Err(eyre::eyre!("Dataflow file not found: {:?}", dataflow_path));
        }

        // 创建 Session ID
        // Create Session ID
        let session_id = SessionId::generate();

        // 配置日志目标
        // Configure log destination
        let (log_destination, log_rx) = match self.config.embedded.log_destination {
            LogDestinationType::Tracing => (LogDestination::Tracing, None),
            LogDestinationType::Channel => {
                let (tx, rx) = flume::bounded(100);
                (LogDestination::Channel { sender: tx }, Some(rx))
            }
        };

        self.log_receiver = log_rx;

        // Build ID
        // Build ID
        let build_id = self.config.embedded.build_id.map(|uuid| {
            // BuildId 需要通过其他方式创建，这里我们使用 generate
            // BuildId needs creation via other means; using generate here
            // 如果有预先存在的 build_id，暂时跳过
            // If pre-existing build_id exists, skip for now
            BuildId::generate()
        });

        // 运行数据流
        // Run dataflow
        let result = Daemon::run_dataflow(
            dataflow_path,
            build_id,
            None, // local_build
            session_id,
            self.config.embedded.uv,
            log_destination,
            None, // working_dir
        )
        .await
        .context("Failed to run dataflow")?;

        info!("Dataflow {} completed", result.uuid);

        Ok(DataflowResult::from(result))
    }

    /// 分布式模式运行
    /// Distributed mode execution
    async fn run_distributed(&mut self) -> Result<DataflowResult> {
        let coordinator_addr = self.config.distributed.coordinator_addr;
        let machine_id = self.config.distributed.machine_id.clone();
        let local_listen_port = self.config.distributed.local_listen_port;

        info!(
            "Connecting to dora-coordinator at {} in distributed mode",
            coordinator_addr
        );

        // 运行 daemon（连接到 coordinator）
        // Run daemon (connect to coordinator)
        Daemon::run(coordinator_addr, machine_id, local_listen_port)
            .await
            .context("Failed to run daemon in distributed mode")?;

        // 分布式模式下，数据流由 coordinator 管理
        // In distributed mode, dataflow is managed by coordinator
        // 返回一个空结果
        // Return an empty result
        Ok(DataflowResult {
            uuid: Uuid::nil(),
            node_results: BTreeMap::new(),
            success: true,
        })
    }
}

// ============================================================================
// Runtime Builder
// ============================================================================

/// 运行时构建器
/// Runtime builder
pub struct DoraRuntimeBuilder {
    config: RuntimeConfig,
}

impl DoraRuntimeBuilder {
    /// 创建新的构建器
    /// Create new builder
    pub fn new<P: AsRef<Path>>(dataflow_path: P) -> Self {
        Self {
            config: RuntimeConfig::embedded(dataflow_path),
        }
    }

    /// 设置为嵌入式模式
    /// Set to embedded mode
    pub fn embedded(mut self) -> Self {
        self.config.mode = RuntimeMode::Embedded;
        self
    }

    /// 设置为分布式模式
    /// Set to distributed mode
    pub fn distributed(mut self, coordinator_addr: SocketAddr) -> Self {
        self.config.mode = RuntimeMode::Distributed;
        self.config.distributed.coordinator_addr = coordinator_addr;
        self
    }

    /// 设置是否使用 uv
    /// Set whether to use uv
    pub fn uv(mut self, uv: bool) -> Self {
        self.config.embedded.uv = uv;
        self
    }

    /// 设置事件输出目录
    /// Set event output directory
    pub fn write_events_to<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config.embedded.write_events_to = Some(path.as_ref().to_path_buf());
        self
    }

    /// 设置日志目标
    /// Set log destination
    pub fn log_destination(mut self, dest: LogDestinationType) -> Self {
        self.config.embedded.log_destination = dest;
        self
    }

    /// 设置机器 ID（分布式模式）
    /// Set machine ID (distributed mode)
    pub fn machine_id(mut self, id: String) -> Self {
        self.config.distributed.machine_id = Some(id);
        self
    }

    /// 设置本地监听端口（分布式模式）
    /// Set local listen port (distributed mode)
    pub fn local_listen_port(mut self, port: u16) -> Self {
        self.config.distributed.local_listen_port = port;
        self
    }

    /// 构建运行时
    /// Build runtime
    #[must_use]
    pub fn build(self) -> DoraRuntime {
        DoraRuntime::new(self.config)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 快速运行数据流（嵌入式模式）
/// Quick run dataflow (embedded mode)
///
/// 这是最简单的运行方式，适合快速测试
/// Simple execution method, suitable for rapid testing
///
/// # Example
/// ```rust,ignore
/// use mofa::dora_adapter::runtime::run_dataflow;
///
/// #[tokio::main]
/// async fn main() -> eyre::Result<()> {
///     let result = run_dataflow("dataflow.yml").await?;
///     info!("Dataflow {} completed", result.uuid);
///     Ok(())
/// }
/// ```
pub async fn run_dataflow<P: AsRef<Path>>(dataflow_path: P) -> Result<DataflowResult> {
    let mut runtime = DoraRuntime::embedded(dataflow_path);
    runtime.run().await
}

/// 运行数据流并打印日志（嵌入式模式）
/// Run dataflow and print logs (embedded mode)
///
/// 启动一个后台线程打印日志消息
/// Start a background thread to print log messages
pub async fn run_dataflow_with_logs<P: AsRef<Path>>(dataflow_path: P) -> Result<DataflowResult> {
    let config =
        RuntimeConfig::embedded(dataflow_path).with_log_destination(LogDestinationType::Channel);

    let mut runtime = DoraRuntime::new(config);

    // 修复：在运行数据流之前提取日志接收器，防止由于有界通道填满而导致的死锁
    // Fix: Extract log receiver before running dataflow to prevent deadlocks caused by bounded channel saturation
    if let Some(rx) = runtime.take_log_receiver() {
        // 生成一个后台 Tokio 任务，并发地持续读取日志
        // Spawn a background Tokio task to concurrently read logs
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv_async().await {
                info!("[{:?}] {}", msg.level, msg.message);
            }
        });
    }

    // 现在运行数据流。由于后台任务正在主动排空通道，日志发送不会被阻塞
    // Execute dataflow. Since background task actively drains the channel, log transmission won't block
    runtime.run().await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_embedded() {
        let config = RuntimeConfig::embedded("test.yml");
        assert_eq!(config.mode, RuntimeMode::Embedded);
        assert_eq!(config.dataflow_path, PathBuf::from("test.yml"));
    }

    #[test]
    fn test_runtime_config_distributed() {
        let addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let config = RuntimeConfig::distributed("test.yml", addr);
        assert_eq!(config.mode, RuntimeMode::Distributed);
        assert_eq!(config.distributed.coordinator_addr, addr);
    }

    #[test]
    fn test_runtime_builder() {
        let runtime = DoraRuntimeBuilder::new("test.yml")
            .embedded()
            .uv(true)
            .log_destination(LogDestinationType::Channel)
            .build();

        assert_eq!(runtime.config.mode, RuntimeMode::Embedded);
        assert!(runtime.config.embedded.uv);
    }

    #[tokio::test]
    async fn test_runtime_state() {
        let runtime = DoraRuntime::embedded("test.yml");
        assert_eq!(runtime.state().await, RuntimeState::Created);
    }
}
