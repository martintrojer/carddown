use super::{Algorithm, CardState, Quality};
use crate::db::GlobalState;

pub struct Simple8 {}

impl Algorithm for Simple8 {
    fn update_state(&self, quality: &Quality, state: &mut CardState, global: &mut GlobalState) {
        if quality.failed() {
            state.repetitions = 0;
            state.interval = 0;
            state.failed_count += 1;
        } else if state.repetitions == 0 || state.interval == 0 {
            state.interval = super::safe_f64_to_u64(first_interval(state.failed_count));
            state.repetitions += 1;
        } else {
            let q = global.mean_q.unwrap_or((*quality as usize) as f64);
            let factor = interval_factor(quality_to_ease(q), state.repetitions);
            let new_interval = state.interval as f64 * factor;
            state.interval = super::safe_f64_to_u64(new_interval.round());
            state.repetitions += 1;
        }
    }
    fn name(&self) -> &'static str {
        "Simple8"
    }
}

/// Returns optimal first interval for a card that has failed `total_failures` times.
fn first_interval(total_failures: u64) -> f64 {
    2.4849 * std::f64::consts::E.powf(-0.057 * total_failures as f64)
}

fn interval_factor(ease: f64, repetitions: u64) -> f64 {
    let r = repetitions as f64;
    // Prevent log2(0) which would return -∞
    let log_r = if r > 0.0 { r.log2() } else { 0.0 };
    1.2 + (ease - 1.2) * 0.5_f64.powf(log_r)
}

fn quality_to_ease(q: f64) -> f64 {
    let q2 = q * q;
    let q3 = q2 * q;
    let q4 = q3 * q;
    
    // Use mul_add for better accuracy: a.mul_add(b, c) computes a * b + c
    q4.mul_add(0.0542, 
        q3.mul_add(-0.4848, 
            q2.mul_add(1.4916, 
                q.mul_add(-1.2403, 1.4515))))
}

#[cfg(test)]
mod tests {

    use crate::algorithm::update_meanq;

    use super::*;

    #[test]
    fn test_simple8() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let simple8 = Simple8 {};

        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 2);
        assert_eq!(state.repetitions, 1);

        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 12);
        assert_eq!(state.repetitions, 2);

        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 42);
        assert_eq!(state.repetitions, 3);

        update_meanq(&mut global, Quality::IncorrectAndForgotten);
        simple8.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
    }

    #[test]
    fn test_simple8_corner_cases() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let simple8 = Simple8 {};

        // Test first interval with multiple failures
        state.failed_count = 5;
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert!(state.interval > 0 && state.interval < 2); // Should be reduced due to failures

        // Test very high repetition count
        state = CardState::default();
        state.repetitions = 20;
        state.interval = 100;
        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert!(state.interval > 100); // Should still increase but at a slower rate

        // Test boundary case with zero interval
        state = CardState::default();
        state.repetitions = 1;
        state.interval = 0;
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert!(state.interval > 0); // Should set a positive interval

        // Test consecutive failures
        state = CardState::default();
        state.interval = 10;
        state.repetitions = 3;
        simple8.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
        simple8.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
    }

    #[test]
    fn test_first_interval() {
        // Test with no failures
        assert_eq!(first_interval(0).round(), 2.0);

        // Test with increasing failures
        assert!(first_interval(1) < first_interval(0));
        assert!(first_interval(5) < first_interval(1));
        assert!(first_interval(10) < first_interval(5));

        // Test that interval never goes below 0
        assert!(first_interval(100) > 0.0);
    }

    #[test]
    fn test_interval_factor() {
        // Test with different ease values
        assert!(interval_factor(2.0, 1) > 1.2);
        assert!(interval_factor(3.0, 1) > interval_factor(2.0, 1));

        // Test with increasing repetitions
        let ease = 2.5;
        let factor1 = interval_factor(ease, 1);
        let factor2 = interval_factor(ease, 2);
        let factor3 = interval_factor(ease, 3);
        assert!(factor2 < factor1); // Factor should decrease with more repetitions
        assert!(factor3 < factor2);

        // Test edge case: repetitions = 0 (should not panic)
        let factor_zero = interval_factor(2.0, 0);
        assert!(factor_zero.is_finite());
        assert_eq!(factor_zero, 2.0); // When log_r = 0, 0.5^0 = 1, so factor = 1.2 + (ease - 1.2) * 1 = ease
    }

    #[test]
    fn test_quality_to_ease() {
        // Test boundary values
        assert!(quality_to_ease(0.0) > 1.0);
        assert!(quality_to_ease(5.0) > quality_to_ease(0.0));

        // Test monotonic increase
        let e1 = quality_to_ease(1.0);
        let e2 = quality_to_ease(2.0);
        let e3 = quality_to_ease(3.0);
        let e4 = quality_to_ease(4.0);
        assert!(e2 > e1);
        assert!(e3 > e2);
        assert!(e4 > e3);
    }

    #[test]
    fn test_global_state_interaction() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let simple8 = Simple8 {};

        // Test with no mean_q set
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert!(state.interval > 0);

        // Test with mean_q set
        update_meanq(&mut global, Quality::CorrectWithDifficulty);
        state = CardState::default();
        state.repetitions = 1;
        state.interval = 10;
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert!(state.interval > 10);
    }
}
