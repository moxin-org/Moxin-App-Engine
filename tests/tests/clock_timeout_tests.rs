use mofa_testing::clock::MockClock;
use std::time::Duration;

#[test]
fn test_clock_timeout_helpers() {
    let clock = MockClock::starting_at(Duration::from_millis(100));
    
    // Test peek
    assert_eq!(clock.peek_millis(), 100);
    
    // Test deadline creation
    let deadline = clock.deadline_after(Duration::from_millis(50));
    assert_eq!(deadline, 150);
    
    // Test has_reached_deadline and remaining_until (not reached)
    assert!(!clock.has_reached_deadline(deadline));
    assert_eq!(clock.remaining_until(deadline), Duration::from_millis(50));
    
    // Advance half way
    clock.advance(Duration::from_millis(25));
    assert_eq!(clock.peek_millis(), 125);
    assert!(!clock.has_reached_deadline(deadline));
    assert_eq!(clock.remaining_until(deadline), Duration::from_millis(25));
    
    // Advance to deadline
    clock.advance(Duration::from_millis(25));
    assert_eq!(clock.peek_millis(), 150);
    assert!(clock.has_reached_deadline(deadline));
    assert_eq!(clock.remaining_until(deadline), Duration::ZERO);
    
    // Advance past deadline
    clock.advance(Duration::from_millis(10));
    assert_eq!(clock.peek_millis(), 160);
    assert!(clock.has_reached_deadline(deadline));
    assert_eq!(clock.remaining_until(deadline), Duration::ZERO);
}

#[test]
fn test_clock_auto_advance_peek_safety() {
    let clock = MockClock::starting_at(Duration::from_millis(100));
    clock.set_auto_advance(Duration::from_millis(10));
    
    // Peek shouldn't trigger auto-advance
    assert_eq!(clock.peek_millis(), 100);
    assert_eq!(clock.peek_millis(), 100);
    
}
