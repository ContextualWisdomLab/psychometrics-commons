//! Contract tests for the review-only CEFR consumer boundary.

use psychometrics_commons_runtime::cefr_language_assessment::{
    CefrActivityDomain, CefrClaimStatus, CefrContractPin, CefrProfileError,
    CefrResultValidationInput, EnglishA1B2PlacementProfile,
    CEFR_ASSESSMENT_BLUEPRINT_SCHEMA_DIGEST, CEFR_LANGUAGE_ASSESSMENT_CONTRACT_REPOSITORY,
    CEFR_LANGUAGE_ASSESSMENT_CONTRACT_VERSION, CEFR_LANGUAGE_ASSESSMENT_DRAFT_COMMIT,
    CEFR_RESULT_SNAPSHOT_SCHEMA_DIGEST, CEFR_TASK_SPECIFICATION_SCHEMA_DIGEST,
};

fn profile() -> EnglishA1B2PlacementProfile {
    EnglishA1B2PlacementProfile::new(
        "instrument_release_english_a1_b2_v1",
        "assessment_blueprint_english_a1_b2_v1",
        "scoring_profile_english_cefr_v1",
        "cut_score_revision_english_a1_b2_v1",
    )
    .unwrap()
}

fn valid_input(profile: &EnglishA1B2PlacementProfile) -> CefrResultValidationInput<'_> {
    CefrResultValidationInput {
        result_ref: "result_snapshot_english_a1_b2_alpha",
        contract_version: CEFR_LANGUAGE_ASSESSMENT_CONTRACT_VERSION,
        assessment_blueprint_ref: profile.assessment_blueprint_ref(),
        result_schema_digest: CEFR_RESULT_SNAPSHOT_SCHEMA_DIGEST,
        schema_validation_ref: "schema_validation_learning_contracts_alpha",
        measured_domains: profile.required_domains(),
        claim_status: CefrClaimStatus::CefrAligned,
        overall_result_reported: false,
    }
}

#[test]
fn draft_pin_contains_exact_upstream_commit_and_schema_digests() {
    let pin = CefrContractPin::draft_pr_five_review_pin();
    assert_eq!(
        pin.repository(),
        CEFR_LANGUAGE_ASSESSMENT_CONTRACT_REPOSITORY
    );
    assert_eq!(pin.commit(), CEFR_LANGUAGE_ASSESSMENT_DRAFT_COMMIT);
    assert_eq!(
        pin.assessment_blueprint_schema_digest(),
        CEFR_ASSESSMENT_BLUEPRINT_SCHEMA_DIGEST
    );
    assert_eq!(
        pin.task_specification_schema_digest(),
        CEFR_TASK_SPECIFICATION_SCHEMA_DIGEST
    );
    assert_eq!(
        pin.result_snapshot_schema_digest(),
        CEFR_RESULT_SNAPSHOT_SCHEMA_DIGEST
    );
    assert!(CEFR_LANGUAGE_ASSESSMENT_DRAFT_COMMIT.len() == 40);
}

#[test]
fn profile_exposes_four_stable_domains_and_product_references() {
    let profile = profile();
    assert_eq!(
        profile.contract_pin(),
        CefrContractPin::draft_pr_five_review_pin()
    );
    assert_eq!(
        profile.instrument_release_ref(),
        "instrument_release_english_a1_b2_v1"
    );
    assert_eq!(
        profile.scoring_profile_ref(),
        "scoring_profile_english_cefr_v1"
    );
    assert_eq!(
        profile.cut_score_revision_ref(),
        "cut_score_revision_english_a1_b2_v1"
    );
    assert_eq!(profile.claim_status(), CefrClaimStatus::CefrAligned);
    assert_eq!(
        profile
            .required_domains()
            .iter()
            .map(|domain| domain.code())
            .collect::<Vec<_>>(),
        vec![
            "reading_reception",
            "listening_reception",
            "written_production",
            "spoken_production"
        ]
    );
}

#[test]
fn profile_rejects_blank_or_unsafe_references() {
    let cases = [
        (
            " ",
            "assessment_blueprint_alpha",
            "scoring_profile_alpha",
            "cut_score_alpha",
        ),
        (
            "instrument_release_alpha",
            " ",
            "scoring_profile_alpha",
            "cut_score_alpha",
        ),
        (
            "instrument_release_alpha",
            "assessment_blueprint_alpha",
            " ",
            "cut_score_alpha",
        ),
        (
            "instrument_release_alpha",
            "assessment_blueprint_alpha",
            "scoring_profile_alpha",
            " ",
        ),
        (
            "instrument_release_alpha",
            "assessment_blueprint_alpha",
            "scoring_profile_alpha",
            "cut_score_\u{200b}alpha",
        ),
    ];
    for (instrument, blueprint, scoring, cut_score) in cases {
        assert_eq!(
            EnglishA1B2PlacementProfile::new(instrument, blueprint, scoring, cut_score),
            Err(CefrProfileError::InvalidReference)
        );
    }
}

#[test]
fn result_validation_accepts_exact_profile_only_result() {
    assert!(profile().validate_result(valid_input(&profile())).is_ok());
}

#[test]
fn result_validation_rejects_contract_identity_and_evidence_mismatches() {
    let profile = profile();
    let mut input = valid_input(&profile);
    input.contract_version = "cwl_cefr_language_assessment/result_snapshot/v2";
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::ContractVersionMismatch)
    );

    let mut input = valid_input(&profile);
    input.assessment_blueprint_ref = "assessment_blueprint_other";
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::BlueprintMismatch)
    );

    let mut input = valid_input(&profile);
    input.result_schema_digest = CEFR_TASK_SPECIFICATION_SCHEMA_DIGEST;
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::ResultSchemaDigestMismatch)
    );

    let mut input = valid_input(&profile);
    input.result_ref = " ";
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::InvalidReference)
    );

    let mut input = valid_input(&profile);
    input.schema_validation_ref = " ";
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::MissingSchemaValidationReference)
    );
}

#[test]
fn result_validation_rejects_incomplete_claims_and_overall_reporting() {
    let profile = profile();
    let mut input = valid_input(&profile);
    input.measured_domains = &profile.required_domains()[..3];
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::InvalidRequiredDomainSet)
    );

    let mut input = valid_input(&profile);
    input.measured_domains = &[
        CefrActivityDomain::ReadingReception,
        CefrActivityDomain::ListeningReception,
        CefrActivityDomain::WrittenProduction,
        CefrActivityDomain::ReadingReception,
    ];
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::InvalidRequiredDomainSet)
    );

    for claim_status in [
        CefrClaimStatus::Experimental,
        CefrClaimStatus::CefrLinked,
        CefrClaimStatus::CertificationDecision,
    ] {
        let mut input = valid_input(&profile);
        input.claim_status = claim_status;
        assert_eq!(
            profile.validate_result(input),
            Err(CefrProfileError::UnsupportedClaimStatus)
        );
    }

    let mut input = valid_input(&profile);
    input.overall_result_reported = true;
    assert_eq!(
        profile.validate_result(input),
        Err(CefrProfileError::OverallReportingDisabled)
    );
}

#[test]
fn profile_errors_have_stable_messages() {
    for error in [
        CefrProfileError::InvalidReference,
        CefrProfileError::ContractVersionMismatch,
        CefrProfileError::BlueprintMismatch,
        CefrProfileError::ResultSchemaDigestMismatch,
        CefrProfileError::MissingSchemaValidationReference,
        CefrProfileError::InvalidRequiredDomainSet,
        CefrProfileError::UnsupportedClaimStatus,
        CefrProfileError::OverallReportingDisabled,
    ] {
        assert!(!error.to_string().is_empty());
        assert!(std::error::Error::source(&error).is_none());
    }
}
