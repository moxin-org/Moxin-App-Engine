use std::time::Duration;

use mofa_testing::clock::{Clock, MockClock};

#[test]
fn deadline_after_uses_current_time() {
    let clock = MockClock::starting_at(Duration::from_millis(1_000));
    let deadline = clock.deadline_after(Duration::from_millis(250));
    assert_eq!(deadline, 1_250);
}

#[test]
fn has_reached_deadline_respects_manual_advance() {
    let clock = MockClock::new();
    let deadline = clock.deadline_after(Duration::from_millis(100));

    assert!(!clock.has_reached_deadline(deadline));
    clock.advance(Duration::from_millis(99));
    assert!(!clock.has_reached_deadline(deadline));
    clock.advance(Duration::from_millis(1));
    assert!(clock.has_reached_deadline(deadline));
}

#[test]
fn remaining_until_is_zero_after_deadline() {
    let clock = MockClock::starting_at(Duration::from_millis(500));
    let deadline = clock.deadline_after(Duration::from_millis(10));

    assert_eq!(clock.remaining_until(deadline), Duration::from_millis(10));
    clock.advance(Duration::from_millis(7));
    assert_eq!(clock.remaining_until(deadline), Duration::from_millis(3));
    clock.advance(Duration::from_millis(10));
    assert_eq!(clock.remaining_until(deadline), Duration::ZERO);
}

#[test]
fn peek_millis_does_not_trigger_auto_advance() {
    let clock = MockClock::new();
    clock.set_auto_advance(Duration::from_millis(50));

    assert_eq!(clock.peek_millis(), 0);
    assert_eq!(clock.peek_millis(), 0);

    // now_millis reads current value and then auto-advances.
    assert_eq!(clock.now_millis(), 0);
    assert_eq!(clock.peek_millis(), 50);
}
