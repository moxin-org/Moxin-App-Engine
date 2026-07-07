//! 记忆组件
//! Memory component
//!
//! 定义 Agent 的记忆/状态持久化能力
//! Defines the Agent's memory and state persistence capabilities

use async_trait::async_trait;
use chrono::Utc;
use mofa_kernel::agent::error::AgentError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

pub use mofa_kernel::agent::components::memory::{
    Memory, MemoryItem, MemoryStats, MemoryValue, Message, MessageRole,
};

// Use kernel's AgentResult type
pub use mofa_kernel::agent::AgentResult;

// ============================================================================
// 内存实现
// In-memory implementation
// ============================================================================

/// 简单内存存储
/// Simple in-memory storage
pub struct InMemoryStorage {
    data: HashMap<String, MemoryItem>,
    history: HashMap<String, Vec<Message>>,
}

impl InMemoryStorage {
    /// 创建新的内存存储
    /// Create new in-memory storage
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            history: HashMap::new(),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Memory for InMemoryStorage {
    async fn store(&mut self, key: &str, value: MemoryValue) -> AgentResult<()> {
        let item = MemoryItem::new(key, value);
        self.data.insert(key.to_string(), item);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AgentResult<Option<MemoryValue>> {
        Ok(self.data.get(key).map(|item| item.value.clone()))
    }

    async fn remove(&mut self, key: &str) -> AgentResult<bool> {
        Ok(self.data.remove(key).is_some())
    }

    async fn search(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryItem>> {
        // Word-level keyword search: split the query into individual words and
        // match items whose key or value text contains any of those words.
        // Words shorter than 3 characters are skipped to avoid noise from
        // stop-words like "a", "is", "of".
        let keywords: Vec<String> = query
            .split_whitespace()
            .filter(|w| w.len() >= 3)
            .map(|w| w.to_lowercase())
            .collect();

        let matches_query = |item: &&MemoryItem| -> bool {
            if keywords.is_empty() {
                return false;
            }
            let key_lc = item.key.to_lowercase();
            let value_lc = item.value.as_text().unwrap_or("").to_lowercase();
            keywords
                .iter()
                .any(|kw| key_lc.contains(kw.as_str()) || value_lc.contains(kw.as_str()))
        };

        let mut results: Vec<MemoryItem> = self
            .data
            .values()
            .filter(matches_query)
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        Ok(results)
    }

    async fn clear(&mut self) -> AgentResult<()> {
        self.data.clear();
        Ok(())
    }

    async fn get_history(&self, session_id: &str) -> AgentResult<Vec<Message>> {
        Ok(self.history.get(session_id).cloned().unwrap_or_default())
    }

    async fn add_to_history(&mut self, session_id: &str, message: Message) -> AgentResult<()> {
        self.history
            .entry(session_id.to_string())
            .or_default()
            .push(message);
        Ok(())
    }

    async fn clear_history(&mut self, session_id: &str) -> AgentResult<()> {
        self.history.remove(session_id);
        Ok(())
    }

    async fn stats(&self) -> AgentResult<MemoryStats> {
        let total_messages: usize = self.history.values().map(|v| v.len()).sum();
        Ok(MemoryStats {
            total_items: self.data.len(),
            total_sessions: self.history.len(),
            total_messages,
            memory_bytes: 0, // 简化，不计算实际内存
                             // Simplified, no actual memory calculation
        })
    }

    fn memory_type(&self) -> &str {
        "in-memory"
    }
}

// ============================================================================
// 基于文件的持久化存储实现
// File-based persistent storage implementation
// ============================================================================

/// 基于文件的持久化存储
/// File-based persistent storage
///
/// 文件结构:
/// File structure:
/// ```text
/// memory/
/// ├── data.json              # KV 存储 (MemoryValue items)
///                            # KV storage (MemoryValue items)
/// ├── MEMORY.md              # 长期记忆
///                            # Long-term memory
/// ├── sessions/              # 会话历史
///                            # Session history
/// │   ├── <session_id>.json  # 单个会话的消息历史
///                            # Message history for a single session
/// ├── 2024-01-15.md          # 每日笔记 (YYYY-MM-DD.md)
///                            # Daily notes (YYYY-MM-DD.md)
/// └── ...
/// ```
///
/// # 特性
/// # Features
///
/// - 持久化到磁盘
/// - Persist to disk
/// - 线程安全 (Arc<RwLock<T>>)
/// - Thread-safe (Arc<RwLock<T>>)
/// - 原子文件写入 (临时文件 + rename)
/// - Atomic file writes (temp file + rename)
/// - 懒加载 (启动时从文件加载到内存)
/// - Lazy loading (load from file to memory at startup)
pub struct FileBasedStorage {
    /// 基础目录
    /// Base directory
    base_dir: PathBuf,
    /// memory 目录
    /// memory directory
    memory_dir: PathBuf,
    /// sessions 目录
    /// sessions directory
    sessions_dir: PathBuf,
    /// data.json 文件路径
    /// Path to data.json
    data_file: PathBuf,
    /// MEMORY.md 文件路径 (长期记忆)
    /// Path to MEMORY.md (Long-term memory)
    long_term_file: PathBuf,
    /// 内存数据 (key -> MemoryItem)
    /// In-memory data (key -> MemoryItem)
    data: Arc<RwLock<HashMap<String, MemoryItem>>>,
    /// 会话历史 (session_id -> Vec<Message>)
    /// Session history (session_id -> Vec<Message>)
    sessions: Arc<RwLock<HashMap<String, Vec<Message>>>>,
}

impl FileBasedStorage {
    /// 创建新的基于文件的存储
    /// Create new file-based storage
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `base_dir`: 基础目录，将在其下创建 memory/ 和 memory/sessions/
    /// - `base_dir`: Base dir, will create memory/ and memory/sessions/ under it
    ///
    /// # 示例
    /// # Example
    ///
    /// ```rust,ignore
    /// use mofa_foundation::agent::components::memory::FileBasedStorage;
    ///
    /// let storage = FileBasedStorage::new("/tmp/workspace").await?;
    /// ```
    pub async fn new(base_dir: impl AsRef<Path>) -> AgentResult<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();
        let memory_dir = base_dir.join("memory");
        let sessions_dir = memory_dir.join("sessions");
        let data_file = memory_dir.join("data.json");
        let long_term_file = memory_dir.join("MEMORY.md");

        // 创建目录
        // Create directories
        tokio::fs::create_dir_all(&sessions_dir)
            .await
            .map_err(|e| {
                AgentError::IoError(format!("Failed to create sessions directory: {}", e))
            })?;

        // 加载现有数据
        // Load existing data
        let data = Self::load_data(&data_file).await?;
        let sessions = Self::load_sessions(&sessions_dir).await?;

        Ok(Self {
            base_dir,
            memory_dir,
            sessions_dir,
            data_file,
            long_term_file,
            data: Arc::new(RwLock::new(data)),
            sessions: Arc::new(RwLock::new(sessions)),
        })
    }

    /// 从 data.json 加载数据
    /// Load data from data.json
    async fn load_data(data_file: &Path) -> AgentResult<HashMap<String, MemoryItem>> {
        if !data_file.exists() {
            return Ok(HashMap::new());
        }

        let content = tokio::fs::read_to_string(data_file)
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to read data.json: {}", e)))?;

        if content.trim().is_empty() {
            return Ok(HashMap::new());
        }

        serde_json::from_str(&content).map_err(|e| {
            AgentError::SerializationError(format!("Failed to parse data.json: {}", e))
        })
    }

    /// 从 sessions 目录加载所有会话
    /// Load all sessions from sessions directory
    async fn load_sessions(sessions_dir: &Path) -> AgentResult<HashMap<String, Vec<Message>>> {
        if !sessions_dir.exists() {
            return Ok(HashMap::new());
        }

        let mut sessions = HashMap::new();
        let mut entries = tokio::fs::read_dir(sessions_dir).await.map_err(|e| {
            AgentError::IoError(format!("Failed to read sessions directory: {}", e))
        })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to read session entry: {}", e)))?
        {
            let path = entry.path();

            // 只处理 .json 文件
            // Only process .json files
            if path.extension().and_then(|s: &std::ffi::OsStr| s.to_str()) != Some("json") {
                continue;
            }

            // 从文件名获取 session_id (例如: "session-123.json" -> "session-123")
            // Get session_id from file name (e.g. "session-123.json" -> "session-123")
            let session_id = path
                .file_stem()
                .and_then(|s: &std::ffi::OsStr| s.to_str())
                .ok_or_else(|| {
                    AgentError::IoError(format!("Invalid session file name: {:?}", path))
                })?;

            // 读取会话数据
            // Read session data
            let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                AgentError::IoError(format!("Failed to read session file {:?}: {}", path, e))
            })?;

            let messages: Vec<Message> = serde_json::from_str(&content).map_err(|e| {
                AgentError::SerializationError(format!(
                    "Failed to parse session file {:?}: {}",
                    path, e
                ))
            })?;

            sessions.insert(session_id.to_string(), messages);
        }

        Ok(sessions)
    }

    /// 持久化数据到 data.json
    /// Persist data to data.json
    ///
    /// 使用原子写入: 写入临时文件然后 rename
    /// Uses atomic write: write to temp file then rename
    async fn persist_data(&self) -> AgentResult<()> {
        let data = self.data.read().await;
        let json = serde_json::to_string_pretty(&*data).map_err(|e| {
            AgentError::SerializationError(format!("Failed to serialize data: {}", e))
        })?;
        drop(data);

        // 原子写入: 临时文件 + rename
        // Atomic write: temp file + rename
        let temp_file = self.data_file.with_extension("json.tmp");
        tokio::fs::write(&temp_file, json)
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to write temp data file: {}", e)))?;

        tokio::fs::rename(&temp_file, &self.data_file)
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to rename data file: {}", e)))?;

        Ok(())
    }

    /// 持久化单个会话到文件
    /// Persist single session to file
    async fn persist_session(&self, session_id: &str) -> AgentResult<()> {
        let sessions = self.sessions.read().await;
        let messages = sessions.get(session_id);

        let session_file = self.sessions_dir.join(format!("{}.json", session_id));

        if let Some(messages) = messages {
            // 写入会话数据
            // Write session data
            let json = serde_json::to_string_pretty(messages).map_err(|e| {
                AgentError::SerializationError(format!("Failed to serialize session: {}", e))
            })?;
            drop(sessions);

            // 原子写入
            // Atomic write
            let temp_file = session_file.with_extension("json.tmp");
            tokio::fs::write(&temp_file, json).await.map_err(|e| {
                AgentError::IoError(format!("Failed to write temp session file: {}", e))
            })?;

            tokio::fs::rename(&temp_file, &session_file)
                .await
                .map_err(|e| {
                    AgentError::IoError(format!("Failed to rename session file: {}", e))
                })?;
        } else {
            // 会话不存在，删除文件
            // Session not found, delete file
            drop(sessions);
            if session_file.exists() {
                tokio::fs::remove_file(&session_file).await.map_err(|e| {
                    AgentError::IoError(format!("Failed to remove session file: {}", e))
                })?;
            }
        }

        Ok(())
    }

    /// 获取今日日期字符串 (YYYY-MM-DD)
    /// Get today's date string (YYYY-MM-DD)
    fn today_key() -> String {
        Utc::now().format("%Y-%m-%d").to_string()
    }

    /// 获取今日文件路径 (YYYY-MM-DD.md)
    /// Get today's file path (YYYY-MM-DD.md)
    fn today_file(&self) -> PathBuf {
        self.memory_dir.join(format!("{}.md", Self::today_key()))
    }

    /// 读取今日笔记内容
    /// Read today's notes content
    pub async fn read_today_file(&self) -> AgentResult<String> {
        let today_file = self.today_file();
        if today_file.exists() {
            tokio::fs::read_to_string(&today_file)
                .await
                .map_err(|e| AgentError::IoError(format!("Failed to read today file: {}", e)))
        } else {
            Ok(String::new())
        }
    }

    /// 追加内容到今日笔记
    /// Append content to today's notes
    pub async fn append_today_file(&self, content: &str) -> AgentResult<()> {
        let today_file = self.today_file();
        let final_content = if today_file.exists() {
            let existing = tokio::fs::read_to_string(&today_file)
                .await
                .map_err(|e| AgentError::IoError(format!("Failed to read today file: {}", e)))?;
            format!("{}\n{}", existing, content)
        } else {
            // 新文件，添加日期头部
            // New file, add date header
            let today = Self::today_key();
            format!("# {}\n\n{}", today, content)
        };

        tokio::fs::write(&today_file, final_content)
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to write today file: {}", e)))?;

        Ok(())
    }

    /// 读取长期记忆 (MEMORY.md)
    /// Read long-term memory (MEMORY.md)
    pub async fn read_long_term_file(&self) -> AgentResult<String> {
        if self.long_term_file.exists() {
            tokio::fs::read_to_string(&self.long_term_file)
                .await
                .map_err(|e| AgentError::IoError(format!("Failed to read long-term file: {}", e)))
        } else {
            Ok(String::new())
        }
    }

    /// 写入长期记忆 (MEMORY.md)
    /// Write long-term memory (MEMORY.md)
    pub async fn write_long_term_file(&self, content: &str) -> AgentResult<()> {
        // 确保目录存在
        // Ensure directory exists
        tokio::fs::create_dir_all(&self.memory_dir)
            .await
            .map_err(|e| {
                AgentError::IoError(format!("Failed to create memory directory: {}", e))
            })?;

        tokio::fs::write(&self.long_term_file, content)
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to write long-term file: {}", e)))?;

        Ok(())
    }

    /// 获取最近 N 天的记忆
    /// Get memories from the last N days
    pub async fn get_recent_memories_files(&self, days: u32) -> AgentResult<String> {
        let mut memories = Vec::new();

        for i in 0..days {
            let date = Utc::now() - chrono::Duration::days(i as i64);
            let date_str = date.format("%Y-%m-%d").to_string();
            let file_path = self.memory_dir.join(format!("{}.md", date_str));

            if file_path.exists() {
                let content = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
                    AgentError::IoError(format!(
                        "Failed to read memory file {:?}: {}",
                        file_path, e
                    ))
                })?;
                memories.push(content);
            }
        }

        Ok(memories.join("\n\n---\n\n"))
    }

    /// 列出所有记忆文件 (按日期排序，最新的在前)
    /// List all memory files (sorted by date, newest first)
    async fn list_memory_files(&self) -> AgentResult<Vec<PathBuf>> {
        if !self.memory_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&self.memory_dir)
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to read memory directory: {}", e)))?;
        let mut files = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AgentError::IoError(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n: &std::ffi::OsStr| n.to_str()) {
                // 检查是否匹配 YYYY-MM-DD.md 模式
                // Check if matching YYYY-MM-DD.md pattern
                if Self::is_date_file(name) {
                    files.push(path);
                }
            }
        }

        // 按文件名倒序排序 (最新的在前)
        // Sort descending by name (newest first)
        files.sort_by(|a: &PathBuf, b: &PathBuf| b.cmp(a));
        Ok(files)
    }

    /// 检查文件名是否匹配日期格式 (YYYY-MM-DD.md)
    /// Check if filename matches date format (YYYY-MM-DD.md)
    fn is_date_file(name: &str) -> bool {
        if name.len() != 13 {
            // "2024-01-15.md" = 13 bytes
            return false;
        }
        let bytes = name.as_bytes();
        bytes[4] == b'-' && bytes[7] == b'-' && name.ends_with(".md")
    }

    /// 获取记忆上下文
    /// Get memory context
    pub async fn get_memory_context(&self) -> AgentResult<String> {
        let mut parts = Vec::new();

        // 长期记忆
        // Long-term memory
        let long_term = self.read_long_term_file().await?;
        if !long_term.is_empty() {
            parts.push(format!("## Long-term Memory\n{}", long_term));
        }

        // 今日笔记
        // Today's notes
        let today = self.read_today_file().await?;
        if !today.is_empty() {
            parts.push(format!("## Today's Notes\n{}", today));
        }

        Ok(parts.join("\n\n"))
    }

    /// 读取今日笔记
    /// Read today's notes
    pub async fn read_today(&self) -> AgentResult<String> {
        self.read_today_file().await
    }

    /// 追加今日笔记
    /// Append to today's notes
    pub async fn append_today(&self, content: &str) -> AgentResult<()> {
        self.append_today_file(content).await
    }

    /// 读取长期记忆
    /// Read long-term memory
    pub async fn read_long_term(&self) -> AgentResult<String> {
        self.read_long_term_file().await
    }

    /// 写入长期记忆
    /// Write long-term memory
    pub async fn write_long_term(&self, content: &str) -> AgentResult<()> {
        self.write_long_term_file(content).await
    }

    /// 获取最近记忆
    /// Get recent memories
    pub async fn get_recent_memories(&self, days: u32) -> AgentResult<String> {
        self.get_recent_memories_files(days).await
    }
}

#[async_trait]
impl Memory for FileBasedStorage {
    async fn store(&mut self, key: &str, value: MemoryValue) -> AgentResult<()> {
        let item = MemoryItem::new(key, value);
        {
            let mut data = self.data.write().await;
            data.insert(key.to_string(), item);
        }
        self.persist_data().await?;
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AgentResult<Option<MemoryValue>> {
        let data = self.data.read().await;
        Ok(data.get(key).map(|item| item.value.clone()))
    }

    async fn remove(&mut self, key: &str) -> AgentResult<bool> {
        let removed = {
            let mut data = self.data.write().await;
            data.remove(key).is_some()
        };
        if removed {
            self.persist_data().await?;
        }
        Ok(removed)
    }

    async fn search(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryItem>> {
        // 在内存数据中搜索
        // Search in in-memory data
        let query_lower = query.to_lowercase();
        let mut results: Vec<MemoryItem> = {
            let data = self.data.read().await;
            data.values()
                .filter(|item| {
                    if let Some(text) = item.value.as_text() {
                        text.to_lowercase().contains(&query_lower)
                    } else {
                        false
                    }
                })
                .cloned()
                .collect()
        };

        // 同时在 markdown 文件中搜索
        // Search in markdown files simultaneously
        let memory_files = self.list_memory_files().await?;
        for file_path in memory_files {
            if let Ok(content) = tokio::fs::read_to_string(&file_path).await
                && content.to_lowercase().contains(&query_lower)
            {
                let file_name = file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                results.push(MemoryItem::new(file_name, MemoryValue::text(content)));
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        Ok(results)
    }

    async fn clear(&mut self) -> AgentResult<()> {
        // 清空内存
        // Clear memory
        {
            let mut data = self.data.write().await;
            data.clear();
        }
        {
            let mut sessions = self.sessions.write().await;
            sessions.clear();
        }

        // 删除所有文件
        // Delete all files
        if self.data_file.exists() {
            tokio::fs::remove_file(&self.data_file)
                .await
                .map_err(|e| AgentError::IoError(format!("Failed to remove data file: {}", e)))?;
        }

        if self.sessions_dir.exists() {
            tokio::fs::remove_dir_all(&self.sessions_dir)
                .await
                .map_err(|e| {
                    AgentError::IoError(format!("Failed to remove sessions directory: {}", e))
                })?;
            tokio::fs::create_dir_all(&self.sessions_dir)
                .await
                .map_err(|e| {
                    AgentError::IoError(format!("Failed to recreate sessions directory: {}", e))
                })?;
        }

        Ok(())
    }

    async fn get_history(&self, session_id: &str) -> AgentResult<Vec<Message>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned().unwrap_or_default())
    }

    async fn add_to_history(&mut self, session_id: &str, message: Message) -> AgentResult<()> {
        {
            let mut sessions = self.sessions.write().await;
            sessions
                .entry(session_id.to_string())
                .or_default()
                .push(message);
        }
        self.persist_session(session_id).await?;
        Ok(())
    }

    async fn clear_history(&mut self, session_id: &str) -> AgentResult<()> {
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id);
        }
        self.persist_session(session_id).await?;
        Ok(())
    }

    async fn stats(&self) -> AgentResult<MemoryStats> {
        let data = self.data.read().await;
        let sessions = self.sessions.read().await;

        let total_messages: usize = sessions.values().map(|v| v.len()).sum();
        let memory_bytes = data.len() * std::mem::size_of::<MemoryItem>();

        Ok(MemoryStats {
            total_items: data.len(),
            total_sessions: sessions.len(),
            total_messages,
            memory_bytes,
        })
    }

    fn memory_type(&self) -> &str {
        "file-based"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_basic_operations() {
        let mut storage = InMemoryStorage::new();

        // Test store and retrieve
        storage.store("key1", MemoryValue::text("value1")).await.unwrap();
        let val = storage
            .retrieve("key1")
            .await
            .unwrap()
            .expect("value for 'key1' should exist after storing");
        assert_eq!(val.as_text(), Some("value1"));

        // Test retrieve non-existent key
        let missing = storage.retrieve("missing_key").await.unwrap();
        assert!(missing.is_none());

        // Test overwrite
        storage.store("key1", MemoryValue::text("new_value")).await.unwrap();
        let val2 = storage
            .retrieve("key1")
            .await
            .unwrap()
            .expect("value for 'key1' should exist after overwrite");
        assert_eq!(val2.as_text(), Some("new_value"));

        // Test remove
        let removed = storage.remove("key1").await.unwrap();
        assert!(removed);

        // Verify removal
        let val3 = storage.retrieve("key1").await.unwrap();
        assert!(val3.is_none());

        // Test remove non-existent key
        let removed_again = storage.remove("key1").await.unwrap();
        assert!(!removed_again);
    }
}
