//! Regression tests for server-authoritative item-delivery session binding.
//!
//! Item-delivery evidence must be bound to the exact immutable release pinned by
//! the assessment session, and callers must not be able to forge lifecycle state
//! by passing detached state evidence.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

const RELEASE_A_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RELEASE_B_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EVIDENCE_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn manifest(
    release_ref: &str,
    instrument_version_ref: &str,
    content_digest: &str,
) -> InstrumentReleaseManifest {
    manifest_with_locale_and_items(
        release_ref,
        instrument_version_ref,
        content_digest,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
}

fn manifest_with_locale_and_items(
    release_ref: &str,
    instrument_version_ref: &str,
    content_digest: &str,
    locale: &str,
    item_version_refs: &[&str],
) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        instrument_version_ref,
        "construct_big_five",
        item_version_refs,
        locale,
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        content_digest,
    )
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    published_release_named(
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        RELEASE_A_DIGEST,
    )
}

fn published_release_named(
    release_ref: &str,
    instrument_version_ref: &str,
    content_digest: &str,
) -> InstrumentRelease {
    let mut release = InstrumentRelease::new(
        manifest(release_ref, instrument_version_ref, content_digest),
        10_000,
    )
    .unwrap();
    release
        .apply_command(
            "publication_review_session_authority",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_session_authority",
                "evidence_policy_self_reflection_v1",
                release_ref,
                instrument_version_ref,
                &["item_version_001", "item_version_002"],
                content_digest,
                "ko-KR",
                "intended_use_self_reflection_v1",
                "assessment_spec_big_five_v1",
                "scoring_version_big_five_v1",
                "calibration_big_five_ko_v1",
                Some("norm_version_big_five_ko_v1"),
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
                &["recovery_big_five_ko_v1"],
                &["approval_psychometrics_big_five_ko_v1"],
                PublicationEvidenceStatus::Approved,
            )
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command(
            "publication_publish_session_authority",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn created_session(release: &InstrumentRelease) -> AssessmentSession {
    AssessmentSession::new(
        "session_item_delivery_authority",
        "participant_item_delivery_authority",
        release,
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn delivery_request() -> ItemDeliveryRequest<'static> {
    ItemDeliveryRequest {
        delivery_ref: "delivery_session_authority_001",
        item_version_ref: "item_version_001",
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    }
}

#[test]
fn ledger_rejects_manifest_not_pinned_by_the_assessment_session() {
    let release = published_release();
    let session = created_session(&release);
    let other_manifest = manifest(
        "release_big_five_ko_v2",
        "instrument_version_big_five_ko_v2",
        RELEASE_B_DIGEST,
    );

    assert_eq!(
        ItemDeliveryLedger::from_session(&session, &other_manifest),
        Err(ItemDeliveryError::SessionReleaseMismatch),
        "a session must not bind item delivery to another immutable release"
    );
}

#[test]
fn ledger_rejects_manifest_that_rebinds_the_session_item_set() {
    let release = published_release();
    let session = created_session(&release);
    let rebound_manifest = manifest_with_locale_and_items(
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        RELEASE_A_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002", "item_version_003"],
    );

    assert_eq!(
        ItemDeliveryLedger::from_session(&session, &rebound_manifest),
        Err(ItemDeliveryError::SessionReleaseMismatch),
        "a matching digest must not let a caller enlarge the session item set"
    );
}

#[test]
fn ledger_rejects_each_isolated_session_release_mismatch() {
    let release = published_release();
    let session = created_session(&release);
    let cases = [
        (
            "release_big_five_ko_v2",
            "instrument_version_big_five_ko_v1",
            RELEASE_A_DIGEST,
            "ko-KR",
            &["item_version_001", "item_version_002"][..],
        ),
        (
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v2",
            RELEASE_A_DIGEST,
            "ko-KR",
            &["item_version_001", "item_version_002"],
        ),
        (
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            RELEASE_B_DIGEST,
            "ko-KR",
            &["item_version_001", "item_version_002"],
        ),
        (
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            RELEASE_A_DIGEST,
            "en-US",
            &["item_version_001", "item_version_002"],
        ),
        (
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            RELEASE_A_DIGEST,
            "ko-KR",
            &["item_version_002", "item_version_001"],
        ),
        (
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            RELEASE_A_DIGEST,
            "ko-KR",
            &["item_version_001"],
        ),
    ];

    for (release_ref, version_ref, digest, locale, items) in cases {
        let mismatched =
            manifest_with_locale_and_items(release_ref, version_ref, digest, locale, items);
        assert_eq!(
            ItemDeliveryLedger::from_session(&session, &mismatched),
            Err(ItemDeliveryError::SessionReleaseMismatch),
            "from_session must reject {release_ref}/{version_ref}/{digest}/{locale}/{items:?}"
        );
    }
}

#[test]
fn created_session_cannot_be_presented_as_active_by_the_caller() {
    let release = published_release();
    let session = created_session(&release);
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest())
        .expect("matching manifest should create the ledger");

    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(
        ledger.deliver(&session, delivery_request()),
        Err(ItemDeliveryError::SessionNotActive(SessionState::Created))
    );
    assert!(ledger.is_empty());
}

#[test]
fn only_the_bound_assessment_session_can_operate_the_ledger() {
    let release = published_release();
    let mut session = created_session(&release);
    session
        .apply_command(
            "session_activate_item_delivery_authority",
            1,
            SessionCommand::Activate,
        )
        .unwrap();
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    let other_session = AssessmentSession::new(
        "session_item_delivery_other",
        "participant_item_delivery_authority",
        &release,
        "ko-KR",
        21_000,
    )
    .unwrap();

    assert_eq!(
        ledger.deliver(&other_session, delivery_request()),
        Err(ItemDeliveryError::SessionMismatch)
    );
    assert!(ledger.is_empty());
    assert!(ledger.deliver(&session, delivery_request()).is_ok());
}

#[test]
fn deliver_rejects_same_session_ref_with_different_release_provenance() {
    let release = published_release();
    let mut session = created_session(&release);
    session
        .apply_command(
            "session_activate_item_delivery_authority",
            1,
            SessionCommand::Activate,
        )
        .unwrap();
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    let other_release = published_release_named(
        "release_big_five_ko_v2",
        "instrument_version_big_five_ko_v2",
        RELEASE_B_DIGEST,
    );
    let other_session = AssessmentSession::new(
        "session_item_delivery_authority",
        "participant_item_delivery_authority",
        &other_release,
        "ko-KR",
        21_000,
    )
    .unwrap();

    assert_eq!(
        ledger.deliver(&other_session, delivery_request()),
        Err(ItemDeliveryError::SessionMismatch),
        "exact session_ref reuse must not rebind delivery to another published release"
    );
    assert!(ledger.is_empty());
}
