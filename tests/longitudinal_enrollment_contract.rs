//! Realistic enrollment contracts for Gyeot-collected EMA/ESM programs.
//!
//! A Seoul clinic participant can start a 14-day mood diary only after granting
//! longitudinal observation consent. Work and home membership stay distinct so
//! later TEPP analysis is not forced into one primary group.

use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::longitudinal::{
    EnrollmentState, LongitudinalEnrollment, LongitudinalEnrollmentError,
    LongitudinalEnrollmentInput,
};

fn granted_longitudinal_snapshot() -> psychometrics_commons_runtime::consent::ConsentSnapshot {
    let mut ledger = ConsentLedger::new("participant_clinic_seoul").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_service",
            purpose: ConsentPurpose::ServiceOperation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "service_form_ko_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_724_000_000_000,
        })
        .unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_longitudinal",
            purpose: ConsentPurpose::LongitudinalObservation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "ema_mood_form_ko_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_724_000_100_000,
        })
        .unwrap();
    ledger.snapshot_as("consent_snapshot_ema_seoul").unwrap()
}

fn seoul_mood_enrollment() -> LongitudinalEnrollmentInput<'static> {
    LongitudinalEnrollmentInput {
        enrollment_ref: "enrollment_mood_diary_seoul",
        tenant_ref: "tenant_clinic_seoul",
        participant_ref: "participant_clinic_seoul",
        program_ref: "program_mood_diary_14_day",
        collection_system_ref: "gyeot_collection_seoul",
        membership_context_refs: &["membership_work_clinic", "membership_home_household"],
        enrolled_at_unix_ms: 1_724_000_200_000,
    }
}

#[test]
fn seoul_clinic_ema_enrolls_after_longitudinal_consent_with_distinct_memberships() {
    let snapshot = granted_longitudinal_snapshot();
    assert_eq!(
        snapshot.active_granted_at(ConsentPurpose::LongitudinalObservation),
        Some(1_724_000_100_000)
    );

    let enrollment = LongitudinalEnrollment::enroll(seoul_mood_enrollment(), &snapshot).unwrap();

    assert_eq!(enrollment.enrollment_ref(), "enrollment_mood_diary_seoul");
    assert_eq!(enrollment.tenant_ref(), "tenant_clinic_seoul");
    assert_eq!(enrollment.participant_ref(), "participant_clinic_seoul");
    assert_eq!(enrollment.program_ref(), "program_mood_diary_14_day");
    assert_eq!(enrollment.collection_system_ref(), "gyeot_collection_seoul");
    assert_eq!(
        enrollment.consent_snapshot_ref(),
        "consent_snapshot_ema_seoul"
    );
    assert_eq!(
        enrollment.membership_context_refs(),
        &[
            "membership_work_clinic".to_owned(),
            "membership_home_household".to_owned()
        ]
    );
    assert_eq!(enrollment.state(), EnrollmentState::Enrolled);
    assert_eq!(enrollment.enrolled_at_unix_ms(), 1_724_000_200_000);
    assert_eq!(enrollment.latest_event_at_unix_ms(), 1_724_000_200_000);
    assert!(enrollment.can_accept_observations());
}

#[test]
fn research_refusal_does_not_block_personal_ema_enrollment() {
    let mut ledger = ConsentLedger::new("participant_clinic_seoul").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_longitudinal",
            purpose: ConsentPurpose::LongitudinalObservation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "ema_mood_form_ko_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_724_000_100_000,
        })
        .unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_research_refuse",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "research_form_ko_v1",
            research_scope_ref: Some("research_scope_big_five_ko"),
            occurred_at_unix_ms: 1_724_000_150_000,
        })
        .unwrap();
    let snapshot = ledger
        .snapshot_as("consent_snapshot_personal_only")
        .unwrap();

    let enrollment = LongitudinalEnrollment::enroll(seoul_mood_enrollment(), &snapshot).unwrap();
    assert_eq!(enrollment.state(), EnrollmentState::Enrolled);
    assert!(enrollment.can_accept_observations());
}

#[test]
fn missing_or_revoked_longitudinal_consent_fails_closed() {
    let mut ledger = ConsentLedger::new("participant_clinic_seoul").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_service",
            purpose: ConsentPurpose::ServiceOperation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "service_form_ko_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_724_000_000_000,
        })
        .unwrap();
    let service_only = ledger.snapshot_as("consent_snapshot_service_only").unwrap();
    assert_eq!(
        LongitudinalEnrollment::enroll(seoul_mood_enrollment(), &service_only),
        Err(LongitudinalEnrollmentError::LongitudinalConsentRequired)
    );

    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_longitudinal",
            purpose: ConsentPurpose::LongitudinalObservation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "ema_mood_form_ko_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_724_000_100_000,
        })
        .unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_longitudinal_revoke",
            purpose: ConsentPurpose::LongitudinalObservation,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "ema_mood_form_ko_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_724_000_180_000,
        })
        .unwrap();
    let revoked = ledger.snapshot_as("consent_snapshot_revoked").unwrap();
    assert_eq!(
        revoked.active_granted_at(ConsentPurpose::LongitudinalObservation),
        None
    );
    assert_eq!(
        LongitudinalEnrollment::enroll(seoul_mood_enrollment(), &revoked),
        Err(LongitudinalEnrollmentError::LongitudinalConsentRequired)
    );
}

#[test]
fn enrollment_rejects_mismatched_participant_early_time_and_duplicate_membership() {
    let snapshot = granted_longitudinal_snapshot();
    let mut other_person = seoul_mood_enrollment();
    other_person.participant_ref = "participant_other_clinic";
    assert_eq!(
        LongitudinalEnrollment::enroll(other_person, &snapshot),
        Err(LongitudinalEnrollmentError::ParticipantMismatch)
    );

    let mut too_early = seoul_mood_enrollment();
    too_early.enrolled_at_unix_ms = 1_724_000_100_000;
    assert_eq!(
        LongitudinalEnrollment::enroll(too_early, &snapshot),
        Err(LongitudinalEnrollmentError::InvalidStartTime)
    );

    let mut zero_time = seoul_mood_enrollment();
    zero_time.enrolled_at_unix_ms = 0;
    assert_eq!(
        LongitudinalEnrollment::enroll(zero_time, &snapshot),
        Err(LongitudinalEnrollmentError::InvalidStartTime)
    );

    let mut duplicate_membership = seoul_mood_enrollment();
    duplicate_membership.membership_context_refs =
        &["membership_work_clinic", " membership_work_clinic "];
    assert_eq!(
        LongitudinalEnrollment::enroll(duplicate_membership, &snapshot),
        Err(LongitudinalEnrollmentError::DuplicateMembershipContext)
    );
}

#[test]
fn blank_or_numeric_enrollment_references_fail_closed() {
    let snapshot = granted_longitudinal_snapshot();
    for (mut input, _label) in [
        (
            LongitudinalEnrollmentInput {
                enrollment_ref: "12",
                ..seoul_mood_enrollment()
            },
            "enrollment",
        ),
        (
            LongitudinalEnrollmentInput {
                tenant_ref: " ",
                ..seoul_mood_enrollment()
            },
            "tenant",
        ),
        (
            LongitudinalEnrollmentInput {
                program_ref: "1.0e3",
                ..seoul_mood_enrollment()
            },
            "program",
        ),
        (
            LongitudinalEnrollmentInput {
                collection_system_ref: "",
                ..seoul_mood_enrollment()
            },
            "collection",
        ),
    ] {
        input.membership_context_refs = &["membership_work_clinic"];
        assert_eq!(
            LongitudinalEnrollment::enroll(input, &snapshot),
            Err(LongitudinalEnrollmentError::EmptyReference)
        );
    }

    let mut blank_membership = seoul_mood_enrollment();
    blank_membership.membership_context_refs = &[" "];
    assert_eq!(
        LongitudinalEnrollment::enroll(blank_membership, &snapshot),
        Err(LongitudinalEnrollmentError::EmptyReference)
    );

    let mut no_membership = seoul_mood_enrollment();
    no_membership.membership_context_refs = &[];
    let enrollment = LongitudinalEnrollment::enroll(no_membership, &snapshot).unwrap();
    assert!(enrollment.membership_context_refs().is_empty());
    assert_eq!(
        enrollment.pause(" ", 1_724_000_300_000),
        Err(LongitudinalEnrollmentError::EmptyReference)
    );
}

#[test]
fn pause_resume_and_withdraw_keep_history_and_reject_illegal_moves() {
    let snapshot = granted_longitudinal_snapshot();
    let enrolled = LongitudinalEnrollment::enroll(seoul_mood_enrollment(), &snapshot).unwrap();

    let paused = enrolled
        .pause("enrollment_event_pause", 1_724_000_300_000)
        .unwrap();
    assert_eq!(paused.state(), EnrollmentState::Paused);
    assert!(!paused.can_accept_observations());
    assert_eq!(paused.latest_event_ref(), Some("enrollment_event_pause"));
    assert_eq!(paused.enrolled_at_unix_ms(), 1_724_000_200_000);

    assert_eq!(
        paused.pause("enrollment_event_pause", 1_724_000_300_000),
        Ok(paused.clone())
    );
    assert_eq!(
        paused.pause("enrollment_event_pause_other", 1_724_000_310_000),
        Err(LongitudinalEnrollmentError::InvalidTransition)
    );
    assert_eq!(
        enrolled.resume("enrollment_event_resume_early", 1_724_000_300_000),
        Err(LongitudinalEnrollmentError::InvalidTransition)
    );
    assert_eq!(
        enrolled.resume("enrollment_event_resume_early", 1_724_000_100_000),
        Err(LongitudinalEnrollmentError::NonMonotonicTimestamp)
    );
    assert_eq!(
        enrolled.withdraw(" ", 1_724_000_300_000),
        Err(LongitudinalEnrollmentError::EmptyReference)
    );

    let resumed = paused
        .resume("enrollment_event_resume", 1_724_000_400_000)
        .unwrap();
    assert_eq!(resumed.state(), EnrollmentState::Enrolled);
    assert!(resumed.can_accept_observations());
    assert_eq!(
        resumed.resume("enrollment_event_resume", 1_724_000_400_000),
        Ok(resumed.clone())
    );

    assert_eq!(
        resumed.pause("enrollment_event_pause_late", 1_724_000_350_000),
        Err(LongitudinalEnrollmentError::NonMonotonicTimestamp)
    );

    let withdrawn = resumed
        .withdraw("enrollment_event_withdraw", 1_724_000_500_000)
        .unwrap();
    assert_eq!(withdrawn.state(), EnrollmentState::Withdrawn);
    assert!(!withdrawn.can_accept_observations());
    assert_eq!(withdrawn.program_ref(), "program_mood_diary_14_day");
    assert_eq!(
        withdrawn.withdraw("enrollment_event_withdraw", 1_724_000_500_000),
        Ok(withdrawn.clone())
    );
    assert_eq!(
        withdrawn.withdraw("enrollment_event_withdraw_other", 1_724_000_600_000),
        Err(LongitudinalEnrollmentError::AlreadyWithdrawn)
    );
    assert_eq!(
        withdrawn.pause("enrollment_event_pause_after", 1_724_000_600_000),
        Err(LongitudinalEnrollmentError::AlreadyWithdrawn)
    );
    assert_eq!(
        withdrawn.resume("enrollment_event_resume_after", 1_724_000_600_000),
        Err(LongitudinalEnrollmentError::AlreadyWithdrawn)
    );
}

#[test]
fn enrollment_error_text_tells_the_operator_the_next_safe_action() {
    assert_eq!(
        LongitudinalEnrollmentError::EmptyReference.to_string(),
        "copy an opaque enrollment, tenant, participant, program, collection-system, membership, or event reference instead of a blank or numeric id"
    );
    assert_eq!(
        LongitudinalEnrollmentError::ParticipantMismatch.to_string(),
        "use the consent snapshot that belongs to this participant"
    );
    assert_eq!(
        LongitudinalEnrollmentError::LongitudinalConsentRequired.to_string(),
        "ask the participant to grant longitudinal observation consent before enrollment"
    );
    assert_eq!(
        LongitudinalEnrollmentError::InvalidStartTime.to_string(),
        "enroll only after the longitudinal consent grant, with a non-zero server time"
    );
    assert_eq!(
        LongitudinalEnrollmentError::DuplicateMembershipContext.to_string(),
        "declare each membership context once; do not collapse duplicates into one group"
    );
    assert_eq!(
        LongitudinalEnrollmentError::InvalidTransition.to_string(),
        "use pause only while enrolled, resume only while paused, and withdraw from an open enrollment"
    );
    assert_eq!(
        LongitudinalEnrollmentError::NonMonotonicTimestamp.to_string(),
        "use a later server time than the last enrollment event"
    );
    assert_eq!(
        LongitudinalEnrollmentError::AlreadyWithdrawn.to_string(),
        "this enrollment is already withdrawn; replay the same withdrawal evidence or start a new enrollment"
    );
}
