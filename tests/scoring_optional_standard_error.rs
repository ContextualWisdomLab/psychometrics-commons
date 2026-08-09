//! Coverage regression for an intentionally absent optional standard error.

use psychometrics_commons_runtime::scoring::ScoreObservation;

#[test]
fn finite_score_may_omit_standard_error_without_becoming_missing() {
    let observation = ScoreObservation::scored("construct_ref", 1.0, None).unwrap();

    assert_eq!(observation.score(), Some(1.0));
    assert_eq!(observation.standard_error(), None);
}
