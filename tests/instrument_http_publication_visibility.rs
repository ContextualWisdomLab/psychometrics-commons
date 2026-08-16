//! Regression coverage for public instrument-catalog publication visibility.
//!
//! The public catalog is an authorization boundary for starting assessments:
//! only exactly `Published` releases may be discoverable. Draft and suspended
//! families must remain indistinguishable from a missing family.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::instrument_http::{
    handle_instrument_http_request, InstrumentHttpRuntime, INSTRUMENT_COLLECTION_PATH,
};

const CONTENT_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn manifest(release_ref: &str, instrument_ref: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        instrument_ref,
        "instrument_version_catalog_visibility_v1",
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
    .unwrap()
}

fn approved_evidence(release_ref: &str, evidence_ref: &str) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        evidence_ref,
        "evidence_policy_catalog_visibility_v1",
        release_ref,
        "instrument_version_catalog_visibility_v1",
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
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_v1"],
        &["approval_psychometrics_big_five_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap()
}

fn published_release(
    release_ref: &str,
    instrument_ref: &str,
    evidence_ref: &str,
) -> InstrumentRelease {
    let mut release =
        InstrumentRelease::new(manifest(release_ref, instrument_ref), 10_000).unwrap();
    release
        .apply_command(
            &format!("publication_review_{release_ref}"),
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence(release_ref, evidence_ref))
        .unwrap();
    release
        .apply_command(
            &format!("publication_publish_{release_ref}"),
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn get_request(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
}

#[test]
fn collection_exposes_only_exactly_published_releases() {
    let visible = published_release(
        "release_visible_published",
        "instrument_visible_family",
        "publication_evidence_visible",
    );
    let mut suspended = published_release(
        "release_hidden_suspended",
        "instrument_suspended_family",
        "publication_evidence_suspended",
    );
    suspended
        .apply_command(
            "publication_suspend_hidden",
            PublicationCommand::Suspend,
            10_300,
        )
        .unwrap();
    let draft = InstrumentRelease::new(
        manifest("release_hidden_draft", "instrument_draft_family"),
        10_000,
    )
    .unwrap();

    let runtime = InstrumentHttpRuntime::new(vec![draft, suspended, visible]);
    let response =
        handle_instrument_http_request(&get_request(INSTRUMENT_COLLECTION_PATH), &runtime);

    assert_eq!(response.status(), 200);
    assert!(response.body().contains("release_visible_published"));
    assert!(!response.body().contains("release_hidden_suspended"));
    assert!(!response.body().contains("release_hidden_draft"));
}

#[test]
fn unpublished_only_families_are_indistinguishable_from_missing_families() {
    let mut suspended = published_release(
        "release_hidden_suspended",
        "instrument_suspended_family",
        "publication_evidence_suspended",
    );
    suspended
        .apply_command(
            "publication_suspend_hidden",
            PublicationCommand::Suspend,
            10_300,
        )
        .unwrap();
    let draft = InstrumentRelease::new(
        manifest("release_hidden_draft", "instrument_draft_family"),
        10_000,
    )
    .unwrap();
    let runtime = InstrumentHttpRuntime::new(vec![draft, suspended]);

    for family in ["instrument_draft_family", "instrument_suspended_family"] {
        let response = handle_instrument_http_request(
            &get_request(&format!("/v1/instruments/{family}")),
            &runtime,
        );
        assert_eq!(response.status(), 404, "{family} must stay undiscoverable");
        assert_eq!(response.content_type(), "application/problem+json");
        assert!(response
            .body()
            .contains("urn:psychometrics-commons:problem:instrument-not-found"));
    }
}
