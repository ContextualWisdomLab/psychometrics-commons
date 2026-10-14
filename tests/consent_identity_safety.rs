//! Identity-boundary regressions for consent and research contribution.

use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ConsentWriteError,
    ResearchContribution,
};

fn research_snapshot() -> psychometrics_commons_runtime::consent::ConsentSnapshot {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 11_000,
        })
        .unwrap();
    ledger.snapshot_as("consent_snapshot_ref").unwrap()
}

#[test]
fn opaque_product_references_reject_numeric_only_identifiers() {
    assert_eq!(
        ConsentLedger::new("12345"),
        Err(ConsentWriteError::EmptyReference)
    );
    assert!(ConsentLedger::new("participant_12345").is_ok());
}

#[test]
fn consent_domain_rejects_surrounding_whitespace_aliases() {
    let padded = [
        " participant_ref ",
        "\u{00a0}participant_ref\u{00a0}",
        "\u{2003}participant_ref\u{2003}",
        "\u{202f}participant_ref\u{202f}",
        "\u{3000}participant_ref\u{3000}",
    ];
    for alias in padded {
        let error = ConsentLedger::new(alias).unwrap_err();
        assert_eq!(
            error.to_string(),
            "consent references must use exact opaque spelling without surrounding whitespace"
        );
    }

    for alias in [" event_ref ", "\u{00a0}event_ref\u{00a0}"] {
        let mut ledger = ConsentLedger::new("participant_ref").unwrap();
        let error = ledger
            .record(ConsentEventInput {
                event_ref: alias,
                purpose: ConsentPurpose::ServiceOperation,
                decision: ConsentDecision::Granted,
                consent_form_version_ref: "service_form_v1",
                research_scope_ref: None,
                occurred_at_unix_ms: 10_000,
            })
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "consent references must use exact opaque spelling without surrounding whitespace"
        );
    }

    for alias in [" form_version_ref ", "\u{2003}form_version_ref\u{2003}"] {
        let mut ledger = ConsentLedger::new("participant_ref").unwrap();
        let error = ledger
            .record(ConsentEventInput {
                event_ref: "event_ref",
                purpose: ConsentPurpose::ServiceOperation,
                decision: ConsentDecision::Granted,
                consent_form_version_ref: alias,
                research_scope_ref: None,
                occurred_at_unix_ms: 10_000,
            })
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "consent references must use exact opaque spelling without surrounding whitespace"
        );
    }

    for alias in [" research_scope ", "\u{202f}research_scope\u{202f}"] {
        let mut ledger = ConsentLedger::new("participant_ref").unwrap();
        let error = ledger
            .record(ConsentEventInput {
                event_ref: "research_event_ref",
                purpose: ConsentPurpose::ResearchContribution,
                decision: ConsentDecision::Granted,
                consent_form_version_ref: "research_form_v1",
                research_scope_ref: Some(alias),
                occurred_at_unix_ms: 10_000,
            })
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "consent references must use exact opaque spelling without surrounding whitespace"
        );
    }

    let ledger = ConsentLedger::new("participant_ref").unwrap();
    let error = ledger.snapshot_as(" consent_snapshot_ref ").unwrap_err();
    assert_eq!(
        error.to_string(),
        "consent references must use exact opaque spelling without surrounding whitespace"
    );
}

#[test]
fn research_contribution_rejects_surrounding_whitespace_aliases() {
    let snapshot = research_snapshot();

    for contribution_ref in [" contribution_ref ", "\u{3000}contribution_ref\u{3000}"] {
        let error = ResearchContribution::from_snapshot(
            contribution_ref,
            "research_participant_ref",
            &snapshot,
            11_000,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "research contribution references must use exact opaque spelling without surrounding whitespace"
        );
    }

    for participant_ref in [
        " research_participant_ref ",
        "\u{00a0}research_participant_ref\u{00a0}",
    ] {
        let error = ResearchContribution::from_snapshot(
            "contribution_ref",
            participant_ref,
            &snapshot,
            11_000,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "research contribution references must use exact opaque spelling without surrounding whitespace"
        );
    }

    let contribution = ResearchContribution::from_snapshot(
        "contribution_ref",
        "research_participant_ref",
        &snapshot,
        11_000,
    )
    .unwrap();
    for withdrawal_ref in [
        " withdrawal_event_ref ",
        "\u{2003}withdrawal_event_ref\u{2003}",
    ] {
        let error = contribution.withdraw(withdrawal_ref, 12_000).unwrap_err();
        assert_eq!(
            error.to_string(),
            "research contribution references must use exact opaque spelling without surrounding whitespace"
        );
    }
}

#[test]
fn research_identity_cannot_reuse_operational_participant_identity() {
    let snapshot = research_snapshot();

    let error = ResearchContribution::from_snapshot(
        "contribution_ref",
        "participant_ref",
        &snapshot,
        11_000,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "research participant reference must differ from the operational participant"
    );
}

#[test]
fn visible_multilingual_references_preserve_exact_spelling() {
    let mut ledger = ConsentLedger::new("참여자_가나다").unwrap();
    let event = ledger
        .record(ConsentEventInput {
            event_ref: "동의_사건_가나다",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "연구_동의서_가나다",
            research_scope_ref: Some("연구_범위_가나다"),
            occurred_at_unix_ms: 11_000,
        })
        .unwrap();
    assert_eq!(event.event_ref(), "동의_사건_가나다");
    assert_eq!(event.consent_form_version_ref(), "연구_동의서_가나다");
    assert_eq!(event.research_scope_ref(), Some("연구_범위_가나다"));

    let snapshot = ledger.snapshot_as("동의_스냅샷_가나다").unwrap();
    assert_eq!(snapshot.snapshot_ref(), "동의_스냅샷_가나다");
    assert_eq!(snapshot.participant_ref(), "참여자_가나다");

    let contribution = ResearchContribution::from_snapshot(
        "연구_기여_가나다",
        "연구_참여자_가나다",
        &snapshot,
        11_000,
    )
    .unwrap();
    assert_eq!(contribution.contribution_ref(), "연구_기여_가나다");
    assert_eq!(
        contribution.research_participant_ref(),
        "연구_참여자_가나다"
    );

    let withdrawn = contribution.withdraw("철회_사건_가나다", 12_000).unwrap();
    assert_eq!(withdrawn.withdrawal_event_ref(), Some("철회_사건_가나다"));
}

#[test]
fn research_identity_rejects_whitespace_padded_aliases() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 11_000,
        })
        .unwrap();
    let snapshot = ledger.snapshot_as("consent_snapshot_ref").unwrap();

    let error = ResearchContribution::from_snapshot(
        "contribution_ref",
        " research_participant_ref ",
        &snapshot,
        11_000,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "research contribution references must use exact opaque spelling without surrounding whitespace"
    );
}
