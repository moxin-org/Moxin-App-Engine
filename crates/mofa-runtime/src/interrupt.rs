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

    // 触发中断
    // Trigger interrupt
    pub fn trigger(&self) {
        self.is_interrupted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_one();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_not_interrupted() {
        let interrupt = AgentInterrupt::new();
        assert!(!interrupt.check());
    }

    #[test]
    fn default_is_not_interrupted() {
        let interrupt = AgentInterrupt::default();
        assert!(!interrupt.check());
    }

    #[test]
    fn trigger_sets_interrupted() {
        let interrupt = AgentInterrupt::new();
        interrupt.trigger();
        assert!(interrupt.check());
    }

    #[test]
    fn reset_clears_interrupted() {
        let interrupt = AgentInterrupt::new();
        interrupt.trigger();
        assert!(interrupt.check());
        interrupt.reset();
        assert!(!interrupt.check());
    }

    #[test]
    fn trigger_reset_trigger_cycle() {
        let interrupt = AgentInterrupt::new();
        interrupt.trigger();
        interrupt.reset();
        interrupt.trigger();
        assert!(interrupt.check());
    }

    #[test]
    fn clone_shares_state() {
        let a = AgentInterrupt::new();
        let b = a.clone();
        a.trigger();
        // Both clones see the same atomic — this is the whole point of Arc
        assert!(b.check());
    }

    #[test]
    fn clone_reset_affects_both() {
        let a = AgentInterrupt::new();
        let b = a.clone();
        a.trigger();
        b.reset();
        assert!(!a.check());
    }

    #[tokio::test]
    async fn notify_wakes_waiter() {
        let interrupt = AgentInterrupt::new();
        let interrupt2 = interrupt.clone();

        let handle = tokio::spawn(async move {
            interrupt2.notify.notified().await;
            interrupt2.check()
        });

        // Small yield to let the spawned task reach the notified().await
        tokio::task::yield_now().await;
        interrupt.trigger();

        let was_interrupted = handle.await.unwrap();
        assert!(was_interrupted);
    }
}
