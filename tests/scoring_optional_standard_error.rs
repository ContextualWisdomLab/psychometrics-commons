//! Coverage regressions for optional scoring standard-error evidence.

use psychometrics_commons_runtime::scoring::ScoreObservation;

#[test]
fn finite_score_may_omit_standard_error_without_becoming_missing() {
    let observation = ScoreObservation::scored("construct_ref", 1.0, None).unwrap();

    assert_eq!(observation.score(), Some(1.0));
    assert_eq!(observation.standard_error(), None);
}

#[test]
fn finite_non_negative_standard_error_is_preserved() {
    let observation = ScoreObservation::scored("construct_ref", 1.0, Some(0.25)).unwrap();

    assert_eq!(observation.score(), Some(1.0));
    assert_eq!(observation.standard_error(), Some(0.25));
}
