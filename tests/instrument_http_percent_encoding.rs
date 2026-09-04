//! Regression coverage for percent-encoded multilingual instrument references.
//!
//! URI path segments are transmitted as percent-encoded UTF-8 by ordinary HTTP
//! clients. The public catalog must decode that transport spelling exactly once,
//! validate the decoded opaque product identity, and reject encoded separators or
//! malformed encodings instead of making multilingual families unreachable.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::instrument_http::{
    handle_instrument_http_request, InstrumentHttpRuntime,
};

const CONTENT_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_multilingual() -> InstrumentRelease {
    let release_ref = "release_multilingual_family_v1";
    let instrument_ref = "instrument_성격_東京";
    let mut release = InstrumentRelease::new(
        InstrumentReleaseManifest::new(
            release_ref,
            instrument_ref,
            "instrument_version_multilingual_v1",
            "construct_big_five",
            &["item_version_001", "item_version_002"],
            "ko-KR",
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_v1",
            Some("norm_version_big_five_v1"),
            "narrative_version_big_five_v1",
            &["consent_service_v1"],
            "intended_use_self_reflection_v1",
            "limitations_nonclinical_v1",
            CONTENT_DIGEST,
        )
        .unwrap(),
        10_000,
    )
    .unwrap();
    release
        .apply_command(
            "publication_review_multilingual",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_multilingual_v1",
                "evidence_policy_multilingual_v1",
                release_ref,
                "instrument_version_multilingual_v1",
                &["item_version_001", "item_version_002"],
                CONTENT_DIGEST,
                "ko-KR",
                "intended_use_self_reflection_v1",
                "assessment_spec_big_five_v1",
                "scoring_version_big_five_v1",
                "calibration_big_five_v1",
                Some("norm_version_big_five_v1"),
                "limitations_nonclinical_v1",
                PublicationEvidenceProvenance::new(
                    EVIDENCE_DIGEST,
                    "population_general_adult_v1",
                    "administration_web_self_report_v1",
                    "measurement_model_big_five_v1",
                    10_050,
                    None,
                )
                .unwrap(),
                &["rights_multilingual_v1"],
                &["recovery_multilingual_v1"],
                &["approval_multilingual_v1"],
                PublicationEvidenceStatus::Approved,
            )
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command(
            "publication_publish_multilingual",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn get(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
}

#[test]
fn percent_encoded_utf8_family_ref_resolves_to_exact_stored_identity() {
    let runtime = InstrumentHttpRuntime::new(vec![published_multilingual()]);

    for path in [
        "/v1/instruments/instrument_%EC%84%B1%EA%B2%A9_%E6%9D%B1%E4%BA%AC",
        "/v1/instruments/instrument_%ec%84%b1%ea%b2%a9_%e6%9d%b1%e4%ba%ac",
    ] {
        let response = handle_instrument_http_request(&get(path), &runtime);
        assert_eq!(response.status(), 200, "{path}");
        assert!(response
            .body()
            .contains("\"instrument_ref\":\"instrument_성격_東京\""));
    }
}

#[test]
fn encoded_separator_and_malformed_percent_escape_fail_closed() {
    let runtime = InstrumentHttpRuntime::new(vec![published_multilingual()]);

    for path in [
        "/v1/instruments/instrument_%EC%84%B1%EA%B2%A9_%E6%9D%B1%E4%BA%AC%2Fextra",
        "/v1/instruments/instrument_%EC%84%B1%EA%B2%A9_%E6%9D%B1%E4%BA%",
        "/v1/instruments/instrument_%EC%84%B1%EA%B2%A9_%E6%9D%B1%E4%BA%A",
        "/v1/instruments/instrument_%EC%84%B1%EA%B2%A9_%E6%9D%B1%E4%BA%ZZ",
        "/v1/instruments/instrument_%FF",
        "/v1/instruments/%20instrument_%EC%84%B1%EA%B2%A9_%E6%9D%B1%E4%BA%AC",
    ] {
        let response = handle_instrument_http_request(&get(path), &runtime);
        assert_eq!(response.status(), 400, "{path}");
        assert_eq!(response.content_type(), "application/problem+json");
    }
}
