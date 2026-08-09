//! Boundary regression for server-authoritative consent event ordering.

use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};

#[test]
fn distinct_consent_events_may_share_the_same_server_timestamp() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    for (event_ref, purpose) in [
        ("service_event", ConsentPurpose::ServiceOperation),
        ("communications_event", ConsentPurpose::Communications),
    ] {
        ledger
            .record(ConsentEventInput {
                event_ref,
                purpose,
                decision: ConsentDecision::Granted,
                consent_form_version_ref: "consent_form_v1",
                research_scope_ref: None,
                occurred_at_unix_ms: 7_000,
            })
            .unwrap();
    }

    assert_eq!(ledger.len(), 2);
}
