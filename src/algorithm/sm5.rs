use super::{new_ease_factor, Algorithm, CardState, EaseFactor, OptimalFactorMatrix, Quality};
use crate::db::GlobalState;

pub struct Sm5 {}

impl Algorithm for Sm5 {
    fn update_state(&self, quality: &Quality, state: &mut CardState, global: &mut GlobalState) {
        let new_ef = new_ease_factor(quality, state.ease_factor);
        let of = get_optimal_factor(
            state.repetitions,
            state.ease_factor,
            &global.optimal_factor_matrix,
        );
        let new_of = new_optimal_factor(of, quality);
        update_optimal_factor_matrix(
            state.repetitions,
            new_ef,
            new_of,
            &mut global.optimal_factor_matrix,
        );
        if quality.failed() {
            state.repetitions = 0;
            state.interval = 0;
        } else {
            state.interval = repetition_interval(
                state.interval,
                state.repetitions,
                new_ef,
                &global.optimal_factor_matrix,
            );
            state.repetitions += 1;
            state.ease_factor = new_ef;
        }
    }
}

fn new_optimal_factor(optimal_factor: f64, quality: &Quality) -> f64 {
    // fraction between 0 and 1 that governs how quickly the spaces between successive
    // repetitions increase, for all items.
    let fraction = 0.5;
    let q = (*quality as usize) as f64;
    let tmp = optimal_factor * (0.72 + (q * 0.07));
    (1.0 - fraction) * optimal_factor + (fraction * tmp)
}

fn update_optimal_factor_matrix(
    repetitions: u64,
    ease_factor: f64,
    optimal_factor: f64,
    of_matrix: &mut OptimalFactorMatrix,
) {
    let mut factors = of_matrix.remove(&repetitions).unwrap_or_default();
    factors.insert(EaseFactor::from_num(ease_factor), optimal_factor);
    of_matrix.insert(repetitions, factors);
}

fn get_optimal_factor(repetitions: u64, ease_factor: f64, of_matrix: &OptimalFactorMatrix) -> f64 {
    of_matrix
        .get(&repetitions)
        .and_then(|factors| factors.get(&EaseFactor::from_num(ease_factor)))
        .copied()
        // Initial value for optimal factor is 4.0
        .unwrap_or_else(|| if repetitions == 0 { 4.0 } else { ease_factor })
}

fn repetition_interval(
    last_interval: u64,
    repetitions: u64,
    ease_factor: f64,
    of_matrix: &OptimalFactorMatrix,
) -> u64 {
    let optimal_factor = get_optimal_factor(repetitions, ease_factor, of_matrix);
    let res = if repetitions == 0 {
        optimal_factor
    } else {
        last_interval as f64 * optimal_factor
    };
    res.round() as u64
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::db::GlobalState;

    #[test]
    fn test_sm5() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let sm5 = Sm5 {};

        sm5.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 4);
        assert_eq!(state.repetitions, 1);
        assert_eq!(state.ease_factor, 2.6);
        assert!(get_optimal_factor(0, 2.6, &global.optimal_factor_matrix) > 4.14);
        assert_eq!(
            get_optimal_factor(1, 5.6, &global.optimal_factor_matrix),
            5.6
        );

        sm5.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 11);
        assert_eq!(state.repetitions, 2);
        assert_eq!(state.ease_factor, 2.7);
        assert!(get_optimal_factor(1, 2.7, &global.optimal_factor_matrix) > 2.691);

        sm5.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 31);
        assert_eq!(state.repetitions, 3);
        assert!(state.ease_factor > 2.8 && state.ease_factor < 2.801);
        let prev_ef = state.ease_factor;

        sm5.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
        assert_eq!(state.ease_factor, prev_ef);
    }
}
