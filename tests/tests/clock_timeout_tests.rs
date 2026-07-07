use std::time::Duration;

use mofa_testing::{Clock, MockClock};

#[test]
fn deadline_after_is_relative_to_current_time() {
    let clock = MockClock::starting_at(Duration::from_millis(250));

    assert_eq!(clock.deadline_after(Duration::from_millis(750)), 1_000);
}

#[test]
fn deadline_helpers_transition_at_boundary() {
    let clock = MockClock::starting_at(Duration::from_millis(1_000));
    let deadline = clock.deadline_after(Duration::from_millis(250));

    assert!(!clock.has_reached_deadline(deadline + 1));
    assert!(!clock.has_reached_deadline(deadline));

    clock.advance(Duration::from_millis(250));
    assert!(clock.has_reached_deadline(deadline));
}

#[test]
fn remaining_until_floors_at_zero() {
    let clock = MockClock::starting_at(Duration::from_millis(900));
    let deadline = clock.deadline_after(Duration::from_millis(100));

    assert_eq!(clock.remaining_until(deadline), Duration::from_millis(100));

    clock.advance(Duration::from_millis(150));
    assert_eq!(clock.remaining_until(deadline), Duration::ZERO);
}

#[test]
fn peek_millis_does_not_auto_advance() {
    let clock = MockClock::starting_at(Duration::from_millis(10));
    clock.set_auto_advance(Duration::from_millis(5));

    assert_eq!(clock.peek_millis(), 10);
    assert_eq!(clock.peek_millis(), 10);
    assert_eq!(clock.now_millis(), 10);
    assert_eq!(clock.peek_millis(), 15);
}