//! HTTP path regression for visible multilingual opaque instrument references.
//!
//! Product references preserve visible multilingual material. The HTTP path
//! representation may percent-encode those UTF-8 bytes; transport decoding must
//! recover the exact opaque reference without accepting encoded path separators,
//! malformed escapes, whitespace aliases, or numeric-like identities.

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
const INSTRUMENT_REF: &str = "instrument_ref_가나다_東京_éclair";
const ENCODED_INSTRUMENT_REF: &str =
    "instrument_ref_%EA%B0%80%EB%82%98%EB%8B%A4_%E6%9D%B1%E4%BA%AC_%C3%A9clair";
const MIXED_CASE_ENCODED_INSTRUMENT_REF: &str =
    "instrument_ref_%EA%B0%80%EB%82%98%EB%8B%A4_%E6%9D%B1%E4%BA%AC_%c3%a9clair";

fn published_multilingual_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_multilingual_catalog_v1",
        INSTRUMENT_REF,
        "instrument_version_multilingual_v1",
        "construct_multilingual_v1",
        &["item_version_multilingual_001"],
        "ko-KR",
        "assessment_spec_multilingual_v1",
        "scoring_version_multilingual_v1",
        "calibration_multilingual_v1",
        Some("norm_version_multilingual_v1"),
        "narrative_version_multilingual_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        CONTENT_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_multilingual_v1",
        "evidence_policy_self_reflection_v1",
        "release_multilingual_catalog_v1",
        "instrument_version_multilingual_v1",
        &["item_version_multilingual_001"],
        CONTENT_DIGEST,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_multilingual_v1",
        "scoring_version_multilingual_v1",
        "calibration_multilingual_v1",
        Some("norm_version_multilingual_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_multilingual_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_multilingual_v1"],
        &["recovery_multilingual_v1"],
        &["approval_psychometrics_multilingual_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_multilingual_v1",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_multilingual_v1",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn request(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
}

#[test]
fn percent_encoded_utf8_family_path_resolves_the_exact_multilingual_reference() {
    let runtime = InstrumentHttpRuntime::new(vec![published_multilingual_release()]);

    for encoded_reference in [ENCODED_INSTRUMENT_REF, MIXED_CASE_ENCODED_INSTRUMENT_REF] {
        let response = handle_instrument_http_request(
            &request(&format!("/v1/instruments/{encoded_reference}")),
            &runtime,
        );

        assert_eq!(response.status(), 200, "{encoded_reference}");
        assert!(response
            .body()
            .contains(&format!("\"instrument_ref\":\"{INSTRUMENT_REF}\"")));
        assert!(response.body().contains("release_multilingual_catalog_v1"));
    }
}

#[test]
fn malformed_or_separator_percent_encoding_fails_closed() {
    let runtime = InstrumentHttpRuntime::new(vec![published_multilingual_release()]);

    for path in [
        "/v1/instruments/instrument_ref_%",
        "/v1/instruments/instrument_ref_%GG",
        "/v1/instruments/instrument_ref_%E3%81",
        "/v1/instruments/instrument_ref_%2Fhidden",
        "/v1/instruments/%20instrument_ref_%EA%B0%80",
    ] {
        let response = handle_instrument_http_request(&request(path), &runtime);
        assert_eq!(response.status(), 400, "{path}");
        assert!(response
            .body()
            .contains("urn:psychometrics-commons:problem:bad-request"));
    }
}
