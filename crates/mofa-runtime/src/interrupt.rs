use std::sync::Arc;
use tokio::sync::Notify;

// 共享中断标记（用于外部触发中断）
// Shared interrupt flag (for triggering interrupts externally)
#[derive(Clone)]
pub struct AgentInterrupt {
    pub notify: Arc<Notify>,
    pub is_interrupted: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for AgentInterrupt {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentInterrupt {
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            is_interrupted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    // 触发中断 — 唤醒所有等待的任务
    // Trigger interrupt — wake ALL waiting tasks
    pub fn trigger(&self) {
        self.is_interrupted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    // 异步等待中断信号
    // Asynchronously wait for an interrupt signal.
    // Uses a loop to handle the race where a task calls wait() after
    // trigger()/notify_waiters() has already drained.
    pub async fn wait(&self) {
        loop {
            if self.is_interrupted
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return;
            }
            self.notify.notified().await;
        }
    }

    // 检查是否中断
    // Check if interrupted
    pub fn check(&self) -> bool {
        self.is_interrupted
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    // 重置中断状态
    // Reset interrupt state
    pub fn reset(&self) {
        self.is_interrupted
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
