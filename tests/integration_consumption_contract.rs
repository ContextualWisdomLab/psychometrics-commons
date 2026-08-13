//! Contract tests for inbox consumption as distinct from receipt.

use psychometrics_commons_runtime::integration::{
    ConsumptionState, InboxConsumption, IntegrationError,
};

fn pending_consumption() -> InboxConsumption {
    InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_session_completed",
        "consumption_session_completed",
        "side_effect_result_projection",
        20_000,
    )
    .unwrap()
}

#[test]
fn pending_consumption_is_not_side_effect_completion() {
    let consumption = pending_consumption();
    assert_eq!(consumption.consumer_ref(), "consumer_alpha");
    assert_eq!(consumption.source_ref(), "psychometrics_commons");
    assert_eq!(consumption.tenant_ref(), "tenant_alpha");
    assert_eq!(consumption.source_event_ref(), "event_session_completed");
    assert_eq!(
        consumption.consumption_ref(),
        "consumption_session_completed"
    );
    assert_eq!(
        consumption.side_effect_ref(),
        "side_effect_result_projection"
    );
    assert_eq!(consumption.state(), ConsumptionState::Pending);
    assert_eq!(consumption.fencing_token(), 0);
    assert_eq!(consumption.latest_event_at_unix_ms(), 20_000);
    assert_eq!(consumption.claim_expires_at_unix_ms(), None);
    assert_eq!(consumption.completion_evidence_ref(), None);
    assert_eq!(consumption.cause_code(), None);
}

#[test]
fn pending_consumption_rejects_invalid_identity_and_time() {
    for invalid in ["", "   ", "12345"] {
        assert_eq!(
            InboxConsumption::pending(
                invalid,
                "psychometrics_commons",
                "tenant_alpha",
                "event_session_completed",
                "consumption_session_completed",
                "side_effect_result_projection",
                20_000,
            ),
            Err(IntegrationError::InvalidReference)
        );
        assert_eq!(
            InboxConsumption::pending(
                "consumer_alpha",
                invalid,
                "tenant_alpha",
                "event_session_completed",
                "consumption_session_completed",
                "side_effect_result_projection",
                20_000,
            ),
            Err(IntegrationError::InvalidReference)
        );
        assert_eq!(
            InboxConsumption::pending(
                "consumer_alpha",
                "psychometrics_commons",
                invalid,
                "event_session_completed",
                "consumption_session_completed",
                "side_effect_result_projection",
                20_000,
            ),
            Err(IntegrationError::InvalidReference)
        );
        assert_eq!(
            InboxConsumption::pending(
                "consumer_alpha",
                "psychometrics_commons",
                "tenant_alpha",
                invalid,
                "consumption_session_completed",
                "side_effect_result_projection",
                20_000,
            ),
            Err(IntegrationError::InvalidReference)
        );
        assert_eq!(
            InboxConsumption::pending(
                "consumer_alpha",
                "psychometrics_commons",
                "tenant_alpha",
                "event_session_completed",
                invalid,
                "side_effect_result_projection",
                20_000,
            ),
            Err(IntegrationError::InvalidReference)
        );
        assert_eq!(
            InboxConsumption::pending(
                "consumer_alpha",
                "psychometrics_commons",
                "tenant_alpha",
                "event_session_completed",
                "consumption_session_completed",
                invalid,
                20_000,
            ),
            Err(IntegrationError::InvalidReference)
        );
    }
    assert_eq!(
        InboxConsumption::pending(
            "consumer_alpha",
            "psychometrics_commons",
            "tenant_alpha",
            "event_session_completed",
            "consumption_session_completed",
            "side_effect_result_projection",
            0,
        ),
        Err(IntegrationError::InvalidTimestamp)
    );
}

#[test]
fn local_effect_completes_pending_consumption_with_zero_fence() {
    let mut consumption = pending_consumption();
    assert_eq!(
        consumption.complete(20_001, "completion_projection_applied", 0),
        Ok(ConsumptionState::Completed)
    );
    assert_eq!(consumption.state(), ConsumptionState::Completed);
    assert_eq!(
        consumption.completion_evidence_ref(),
        Some("completion_projection_applied")
    );
    assert_eq!(
        consumption.complete(20_001, "completion_projection_applied", 0),
        Ok(ConsumptionState::Completed)
    );
}

#[test]
fn claimed_worker_completes_only_with_current_fence() {
    let mut consumption = pending_consumption();
    assert_eq!(consumption.begin_processing(20_001, 21_000).unwrap(), 1);
    assert_eq!(consumption.state(), ConsumptionState::Processing);
    assert_eq!(
        consumption.complete(20_002, "completion_projection_applied", 0),
        Err(IntegrationError::StaleConsumptionFence)
    );
    assert_eq!(
        consumption.complete(20_002, "completion_projection_applied", 1),
        Ok(ConsumptionState::Completed)
    );
}

#[test]
fn begin_processing_rejects_non_pending_and_invalid_time() {
    let mut consumption = pending_consumption();
    assert_eq!(
        consumption.begin_processing(0, 21_000),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        consumption.begin_processing(20_001, 0),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        consumption.begin_processing(20_001, 20_001),
        Err(IntegrationError::InvalidConsumptionClaimWindow)
    );
    assert_eq!(
        consumption.begin_processing(19_999, 21_000),
        Err(IntegrationError::NonMonotonicTimestamp)
    );
    consumption.begin_processing(20_001, 21_000).unwrap();
    assert_eq!(
        consumption.begin_processing(20_002, 21_000),
        Err(IntegrationError::ConsumptionNotClaimable)
    );

    let mut completed = pending_consumption();
    completed
        .complete(20_001, "completion_projection_applied", 0)
        .unwrap();
    assert_eq!(
        completed.begin_processing(20_002, 21_000),
        Err(IntegrationError::TerminalConsumptionState)
    );

    let mut quarantined = pending_consumption();
    quarantined.quarantine(20_001, "poison_payload", 0).unwrap();
    assert_eq!(
        quarantined.begin_processing(20_002, 21_000),
        Err(IntegrationError::TerminalConsumptionState)
    );
}

#[test]
fn expire_processing_returns_pending_without_transferring_the_fence() {
    let mut consumption = pending_consumption();
    assert_eq!(consumption.begin_processing(20_001, 21_000).unwrap(), 1);
    assert_eq!(consumption.claim_expires_at_unix_ms(), Some(21_000));
    assert_eq!(
        consumption.expire_processing(20_500),
        Err(IntegrationError::ConsumptionClaimStillActive)
    );
    assert_eq!(
        consumption.expire_processing(21_000),
        Ok(ConsumptionState::Pending)
    );
    assert_eq!(consumption.state(), ConsumptionState::Pending);
    assert_eq!(consumption.fencing_token(), 1);
    assert_eq!(consumption.claim_expires_at_unix_ms(), None);
    assert_eq!(
        consumption.complete(21_001, "completion_projection_applied", 1),
        Err(IntegrationError::StaleConsumptionFence)
    );
    assert_eq!(
        consumption.complete(21_001, "completion_projection_applied", 0),
        Ok(ConsumptionState::Completed)
    );
}

#[test]
fn expire_processing_allows_a_later_claim_with_a_new_fence() {
    let mut consumption = pending_consumption();
    assert_eq!(consumption.begin_processing(20_001, 21_000).unwrap(), 1);
    assert_eq!(
        consumption.expire_processing(21_000),
        Ok(ConsumptionState::Pending)
    );
    assert_eq!(consumption.begin_processing(21_001, 22_000).unwrap(), 2);
    assert_eq!(
        consumption.complete(21_002, "completion_projection_applied", 1),
        Err(IntegrationError::StaleConsumptionFence)
    );
    assert_eq!(
        consumption.complete(21_002, "completion_projection_applied", 2),
        Ok(ConsumptionState::Completed)
    );
}

#[test]
fn expire_processing_rejects_invalid_time_and_non_processing_state() {
    let mut consumption = pending_consumption();
    assert_eq!(
        consumption.expire_processing(0),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        consumption.expire_processing(20_001),
        Err(IntegrationError::ConsumptionNotProcessing)
    );
    consumption.begin_processing(20_001, 21_000).unwrap();
    consumption
        .complete(20_002, "completion_projection_applied", 1)
        .unwrap();
    assert_eq!(
        consumption.expire_processing(21_000),
        Err(IntegrationError::ConsumptionNotProcessing)
    );
}

#[test]
fn complete_and_quarantine_fail_closed_on_conflict_and_terminal_states() {
    let mut consumption = pending_consumption();
    consumption
        .complete(20_001, "completion_projection_applied", 0)
        .unwrap();
    assert_eq!(
        consumption.complete(20_001, "completion_other_evidence", 0),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        consumption.complete(20_002, "completion_projection_applied", 0),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        consumption.complete(20_001, "completion_projection_applied", 1),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        consumption.quarantine(20_002, "poison_payload", 0),
        Err(IntegrationError::TerminalConsumptionState)
    );

    let mut quarantined = pending_consumption();
    assert_eq!(
        quarantined.quarantine(20_001, "poison_payload", 0),
        Ok(ConsumptionState::Quarantined)
    );
    assert_eq!(
        quarantined.quarantine(20_001, "poison_payload", 0),
        Ok(ConsumptionState::Quarantined)
    );
    assert_eq!(
        quarantined.quarantine(20_001, "other_cause", 0),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        quarantined.complete(20_002, "completion_projection_applied", 0),
        Err(IntegrationError::TerminalConsumptionState)
    );
}

#[test]
fn complete_and_quarantine_reject_invalid_inputs_and_stale_or_backward_time() {
    let mut consumption = pending_consumption();
    assert_eq!(
        consumption.complete(0, "completion_projection_applied", 0),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        consumption.complete(20_001, "12345", 0),
        Err(IntegrationError::InvalidReference)
    );
    assert_eq!(
        consumption.complete(19_999, "completion_projection_applied", 0),
        Err(IntegrationError::NonMonotonicTimestamp)
    );
    assert_eq!(
        consumption.complete(20_001, "completion_projection_applied", 1),
        Err(IntegrationError::StaleConsumptionFence)
    );

    let mut claimed = pending_consumption();
    claimed.begin_processing(20_001, 21_000).unwrap();
    assert_eq!(
        claimed.quarantine(0, "poison_payload", 1),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        claimed.quarantine(20_002, "   ", 1),
        Err(IntegrationError::InvalidReference)
    );
    assert_eq!(
        claimed.quarantine(20_000, "poison_payload", 1),
        Err(IntegrationError::NonMonotonicTimestamp)
    );
    assert_eq!(
        claimed.quarantine(20_002, "poison_payload", 0),
        Err(IntegrationError::StaleConsumptionFence)
    );
    assert_eq!(
        claimed.quarantine(20_002, "poison_payload", 1),
        Ok(ConsumptionState::Quarantined)
    );
    assert_eq!(claimed.cause_code(), Some("poison_payload"));
    assert_eq!(
        claimed.quarantine(20_003, "poison_payload", 1),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        claimed.quarantine(20_002, "poison_payload", 2),
        Err(IntegrationError::ConflictingReplay)
    );
}
