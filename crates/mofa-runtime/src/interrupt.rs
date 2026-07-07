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
    use super::AgentInterrupt;
    use std::time::Duration;

    #[test]
    fn test_new_starts_not_interrupted() {
        let interrupt = AgentInterrupt::new();
        assert!(!interrupt.check());
    }

    #[test]
    fn test_default_starts_not_interrupted() {
        let interrupt = AgentInterrupt::default();
        assert!(!interrupt.check());
    }

    #[test]
    fn test_trigger_sets_interrupted_state() {
        let interrupt = AgentInterrupt::new();
        interrupt.trigger();
        assert!(interrupt.check());
    }

    #[test]
    fn test_reset_clears_interrupted_state() {
        let interrupt = AgentInterrupt::new();
        interrupt.trigger();
        interrupt.reset();
        assert!(!interrupt.check());
    }

    #[test]
    fn test_trigger_reset_trigger_cycle() {
        let interrupt = AgentInterrupt::new();

        interrupt.trigger();
        assert!(interrupt.check());

        interrupt.reset();
        assert!(!interrupt.check());

        interrupt.trigger();
        assert!(interrupt.check());
    }

    #[test]
    fn test_clone_shares_triggered_state() {
        let interrupt = AgentInterrupt::new();
        let cloned = interrupt.clone();

        interrupt.trigger();
        assert!(cloned.check());
    }

    #[test]
    fn test_clone_reset_affects_both_instances() {
        let interrupt = AgentInterrupt::new();
        let cloned = interrupt.clone();

        interrupt.trigger();
        assert!(interrupt.check());
        assert!(cloned.check());

        cloned.reset();
        assert!(!interrupt.check());
        assert!(!cloned.check());
    }

    #[tokio::test]
    async fn test_trigger_notifies_waiter() {
        let interrupt = AgentInterrupt::new();
        let notified = interrupt.notify.notified();

        interrupt.trigger();

        let result = tokio::time::timeout(Duration::from_millis(200), notified).await;
        assert!(result.is_ok(), "notify waiter should be woken by trigger");
    }
}
