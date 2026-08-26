//! Error-surface coverage for versioned assessment delivery paths.

use psychometrics_commons_runtime::assessment_path::AssessmentPathError;
use std::error::Error;

#[test]
fn every_path_error_has_stable_beginner_readable_text_and_no_hidden_source() {
    let cases = [
        (
            AssessmentPathError::InvalidReference,
            "assessment path policy reference must be an exact opaque non-numeric value",
        ),
        (
            AssessmentPathError::EmptyItemSet,
            "assessment path must contain at least one item version",
        ),
        (
            AssessmentPathError::DuplicateItemReference,
            "assessment path item-version references must be unique",
        ),
        (
            AssessmentPathError::ItemOutsideRelease,
            "assessment path items must belong to the exact immutable instrument release",
        ),
        (
            AssessmentPathError::ItemOrderMismatch,
            "assessment path items must preserve the immutable instrument release order",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
}
