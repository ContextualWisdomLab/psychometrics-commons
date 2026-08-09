//! Temporal provenance regressions for research contribution opt-in.

use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ResearchContribution,
    ResearchContributionError,
};

fn consent_snapshot() -> psychometrics_commons_runtime::consent::ConsentSnapshot {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 10_000,
        })
        .unwrap();
    ledger.snapshot_as("consent_snapshot_ref").unwrap()
}

#[test]
fn research_contribution_cannot_predate_its_authorizing_consent() {
    let snapshot = consent_snapshot();

    assert_eq!(
        ResearchContribution::from_snapshot(
            "contribution_ref",
            "research_participant_ref",
            &snapshot,
            9_999,
        ),
        Err(ResearchContributionError::InvalidStartTime)
    );
}

#[test]
fn research_contribution_may_start_at_the_authorizing_consent_time() {
    let snapshot = consent_snapshot();

    let contribution = ResearchContribution::from_snapshot(
        "contribution_ref",
        "research_participant_ref",
        &snapshot,
        10_000,
    )
    .unwrap();

    assert_eq!(contribution.started_at_unix_ms(), 10_000);
}
