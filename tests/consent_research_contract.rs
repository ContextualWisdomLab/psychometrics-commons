//! Integration contract for purpose-specific consent and research contribution.

use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ConsentWriteError,
    ResearchContribution, ResearchContributionError, ResearchContributionState,
};

fn grant<'a>(
    event_ref: &'a str,
    purpose: ConsentPurpose,
    form_version_ref: &'a str,
    research_scope_ref: Option<&'a str>,
    occurred_at_unix_ms: u64,
) -> ConsentEventInput<'a> {
    ConsentEventInput {
        event_ref,
        purpose,
        decision: ConsentDecision::Granted,
        consent_form_version_ref: form_version_ref,
        research_scope_ref,
        occurred_at_unix_ms,
    }
}

#[test]
fn service_and_research_consent_are_independent_purposes() {
    let mut ledger = ConsentLedger::new(" participant_ref ").unwrap();
    assert!(ledger.is_empty());
    ledger
        .record(grant(
            "service_grant",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            1_000,
        ))
        .unwrap();

    let snapshot = ledger.snapshot_as(" consent_snapshot_1 ").unwrap();

    assert!(!ledger.is_empty());
    assert_eq!(snapshot.snapshot_ref(), "consent_snapshot_1");
    assert_eq!(snapshot.participant_ref(), "participant_ref");
    assert_eq!(snapshot.event_count(), 1);
    assert!(snapshot.is_granted(ConsentPurpose::ServiceOperation));
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert_eq!(
        snapshot.active_form_version(ConsentPurpose::ResearchContribution),
        None
    );
    assert_eq!(snapshot.active_research_scope(), None);
    assert_eq!(
        ResearchContribution::from_snapshot(
            "research_contribution_1",
            "research_participant_1",
            &snapshot,
            1_100,
        ),
        Err(ResearchContributionError::ResearchConsentRequired)
    );
}

#[test]
fn explicit_research_grant_requires_scope_and_enables_contribution() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(grant(
            "research_grant",
            ConsentPurpose::ResearchContribution,
            "research_form_v2",
            Some(" study_scope_v3 "),
            2_000,
        ))
        .unwrap();

    let snapshot = ledger.snapshot_as("consent_snapshot_2").unwrap();
    let contribution = ResearchContribution::from_snapshot(
        " contribution_ref ",
        " research_participant_ref ",
        &snapshot,
        2_100,
    )
    .unwrap();

    assert_eq!(
        snapshot.active_form_version(ConsentPurpose::ResearchContribution),
        Some("research_form_v2")
    );
    assert_eq!(snapshot.active_research_scope(), Some("study_scope_v3"));
    assert_eq!(contribution.contribution_ref(), "contribution_ref");
    assert_eq!(
        contribution.research_participant_ref(),
        "research_participant_ref"
    );
    assert_eq!(contribution.consent_snapshot_ref(), "consent_snapshot_2");
    assert_eq!(contribution.research_scope_ref(), "study_scope_v3");
    assert_eq!(contribution.state(), ResearchContributionState::Active);
    assert_eq!(contribution.started_at_unix_ms(), 2_100);
    assert_eq!(contribution.withdrawal_event_ref(), None);
    assert_eq!(contribution.withdrawn_at_unix_ms(), None);
}

#[test]
fn research_revocation_is_append_only_and_blocks_new_contributions() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(grant(
            "research_grant",
            ConsentPurpose::ResearchContribution,
            "research_form_v1",
            Some("research_scope_v1"),
            3_000,
        ))
        .unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_revocation",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 3_100,
        })
        .unwrap();

    let snapshot = ledger.snapshot_as("consent_snapshot_3").unwrap();

    assert_eq!(ledger.len(), 2);
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert_eq!(
        snapshot.active_form_version(ConsentPurpose::ResearchContribution),
        None
    );
    assert_eq!(snapshot.active_research_scope(), None);
    assert_eq!(
        ResearchContribution::from_snapshot(
            "contribution_ref",
            "research_participant_ref",
            &snapshot,
            3_200,
        ),
        Err(ResearchContributionError::ResearchConsentRequired)
    );
}

#[test]
fn repeated_identical_consent_event_is_idempotent_but_conflicts_fail_closed() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    let event = grant(
        "research_grant",
        ConsentPurpose::ResearchContribution,
        "research_form_v1",
        Some("research_scope_v1"),
        4_000,
    );

    let first = ledger.record(event).unwrap();
    let replay = ledger.record(event).unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.event_ref(), "research_grant");
    assert_eq!(first.purpose(), ConsentPurpose::ResearchContribution);
    assert_eq!(first.decision(), ConsentDecision::Granted);
    assert_eq!(first.consent_form_version_ref(), "research_form_v1");
    assert_eq!(first.research_scope_ref(), Some("research_scope_v1"));
    assert_eq!(first.occurred_at_unix_ms(), 4_000);
    assert_eq!(ledger.len(), 1);

    assert_eq!(
        ledger.record(grant(
            "research_grant",
            ConsentPurpose::ResearchContribution,
            "research_form_v2",
            Some("research_scope_v1"),
            4_000,
        )),
        Err(ConsentWriteError::EventReferenceConflict)
    );
}

#[test]
fn consent_contract_rejects_blank_invalid_and_mis_scoped_evidence() {
    assert_eq!(
        ConsentLedger::new("  "),
        Err(ConsentWriteError::EmptyReference)
    );

    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    assert_eq!(
        ledger.record(grant(
            " ",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            5_000,
        )),
        Err(ConsentWriteError::EmptyReference)
    );
    assert_eq!(
        ledger.record(grant(
            "event_ref",
            ConsentPurpose::ServiceOperation,
            " ",
            None,
            5_000,
        )),
        Err(ConsentWriteError::EmptyReference)
    );
    assert_eq!(
        ledger.record(grant(
            "event_ref",
            ConsentPurpose::ResearchContribution,
            "research_form_v1",
            None,
            5_000,
        )),
        Err(ConsentWriteError::ResearchScopeRequired)
    );
    assert_eq!(
        ledger.record(grant(
            "event_ref",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            Some("research_scope_v1"),
            5_000,
        )),
        Err(ConsentWriteError::ResearchScopeNotAllowed)
    );
    assert_eq!(
        ledger.record(grant(
            "event_ref",
            ConsentPurpose::ResearchContribution,
            "research_form_v1",
            Some("   "),
            5_000,
        )),
        Err(ConsentWriteError::EmptyReference)
    );
    assert_eq!(
        ledger.record(grant(
            "event_ref",
            ConsentPurpose::Communications,
            "communications_form_v1",
            None,
            0,
        )),
        Err(ConsentWriteError::InvalidTimestamp)
    );
    assert_eq!(
        ledger.snapshot_as("  "),
        Err(ConsentWriteError::EmptyReference)
    );
}

#[test]
fn latest_event_per_purpose_controls_snapshot_without_erasing_history() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(grant(
            "account_grant_v1",
            ConsentPurpose::AccountPersistence,
            "account_form_v1",
            None,
            6_000,
        ))
        .unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "account_revoke_v1",
            purpose: ConsentPurpose::AccountPersistence,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "account_form_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 6_100,
        })
        .unwrap();
    ledger
        .record(grant(
            "account_grant_v2",
            ConsentPurpose::AccountPersistence,
            "account_form_v2",
            None,
            6_200,
        ))
        .unwrap();
    ledger
        .record(grant(
            "longitudinal_grant",
            ConsentPurpose::LongitudinalObservation,
            "longitudinal_form_v1",
            None,
            6_300,
        ))
        .unwrap();

    let snapshot = ledger.snapshot_as("consent_snapshot_6").unwrap();

    assert_eq!(ledger.len(), 4);
    assert!(snapshot.is_granted(ConsentPurpose::AccountPersistence));
    assert_eq!(
        snapshot.active_form_version(ConsentPurpose::AccountPersistence),
        Some("account_form_v2")
    );
    assert!(snapshot.is_granted(ConsentPurpose::LongitudinalObservation));
}

#[test]
fn events_must_be_recorded_in_server_authoritative_time_order() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(grant(
            "first_event",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            7_000,
        ))
        .unwrap();

    assert_eq!(
        ledger.record(grant(
            "older_event",
            ConsentPurpose::Communications,
            "communications_form_v1",
            None,
            6_999,
        )),
        Err(ConsentWriteError::NonMonotonicTimestamp)
    );
}

#[test]
fn contribution_withdrawal_is_monotonic_idempotent_and_irreversible() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(grant(
            "research_grant",
            ConsentPurpose::ResearchContribution,
            "research_form_v1",
            Some("research_scope_v1"),
            8_000,
        ))
        .unwrap();
    let snapshot = ledger.snapshot_as("consent_snapshot_8").unwrap();
    let contribution = ResearchContribution::from_snapshot(
        "contribution_ref",
        "research_participant_ref",
        &snapshot,
        8_100,
    )
    .unwrap();

    let withdrawn = contribution.withdraw(" withdrawal_event ", 8_200).unwrap();
    let replay = withdrawn.withdraw("withdrawal_event", 8_200).unwrap();

    assert_eq!(withdrawn, replay);
    assert_eq!(withdrawn.state(), ResearchContributionState::Withdrawn);
    assert_eq!(withdrawn.withdrawal_event_ref(), Some("withdrawal_event"));
    assert_eq!(withdrawn.withdrawn_at_unix_ms(), Some(8_200));
    assert_eq!(
        withdrawn.withdraw("different_event", 8_300),
        Err(ResearchContributionError::AlreadyWithdrawn)
    );
    assert_eq!(
        contribution.withdraw("withdrawal_event", 8_000),
        Err(ResearchContributionError::InvalidWithdrawalTime)
    );
    assert_eq!(
        contribution.withdraw(" ", 8_200),
        Err(ResearchContributionError::EmptyReference)
    );
}

#[test]
fn contribution_rejects_invalid_identity_and_start_time() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(grant(
            "research_grant",
            ConsentPurpose::ResearchContribution,
            "research_form_v1",
            Some("research_scope_v1"),
            9_000,
        ))
        .unwrap();
    let snapshot = ledger.snapshot_as("consent_snapshot_9").unwrap();

    assert_eq!(
        ResearchContribution::from_snapshot(" ", "research_participant_ref", &snapshot, 9_100),
        Err(ResearchContributionError::EmptyReference)
    );
    assert_eq!(
        ResearchContribution::from_snapshot("contribution_ref", " ", &snapshot, 9_100),
        Err(ResearchContributionError::EmptyReference)
    );
    assert_eq!(
        ResearchContribution::from_snapshot(
            "contribution_ref",
            "research_participant_ref",
            &snapshot,
            0,
        ),
        Err(ResearchContributionError::InvalidStartTime)
    );
}

#[test]
fn public_error_messages_are_stable_and_readable() {
    let consent_errors = [
        (
            ConsentWriteError::EmptyReference,
            "consent references must not be empty",
        ),
        (
            ConsentWriteError::ResearchScopeRequired,
            "research consent requires a research scope",
        ),
        (
            ConsentWriteError::ResearchScopeNotAllowed,
            "research scope is allowed only for research-contribution consent",
        ),
        (
            ConsentWriteError::InvalidTimestamp,
            "consent event timestamp must be greater than zero",
        ),
        (
            ConsentWriteError::EventReferenceConflict,
            "consent event reference was already used for different evidence",
        ),
        (
            ConsentWriteError::NonMonotonicTimestamp,
            "consent event timestamp must not precede the latest accepted event",
        ),
    ];
    for (error, expected) in consent_errors {
        assert_eq!(error.to_string(), expected);
    }

    let research_errors = [
        (
            ResearchContributionError::EmptyReference,
            "research contribution references must not be empty",
        ),
        (
            ResearchContributionError::ResearchConsentRequired,
            "research contribution requires active explicit research consent",
        ),
        (
            ResearchContributionError::InvalidStartTime,
            "research contribution start time must be greater than zero",
        ),
        (
            ResearchContributionError::InvalidWithdrawalTime,
            "research withdrawal time must be later than contribution start",
        ),
        (
            ResearchContributionError::AlreadyWithdrawn,
            "research contribution has already been withdrawn with different evidence",
        ),
    ];
    for (error, expected) in research_errors {
        assert_eq!(error.to_string(), expected);
    }
}
