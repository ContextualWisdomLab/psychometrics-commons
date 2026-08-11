//! Contract tests for operation-scoped runtime readiness and capability health.

use psychometrics_commons_runtime::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, HealthContractError,
    RuntimeHealthSnapshot,
};

fn healthy_snapshot(capabilities: Vec<CapabilityHealth>) -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::WithinBounds,
        DataIntegrityHealth::Verified,
        capabilities,
    )
    .unwrap()
}

#[test]
fn liveness_is_distinct_from_operation_readiness() {
    let snapshot = healthy_snapshot(vec![
        CapabilityHealth::new("persistence", CapabilityState::Unavailable, false).unwrap(),
        CapabilityHealth::new("scoring", CapabilityState::Available, true).unwrap(),
    ]);

    assert!(snapshot.is_live());
    assert!(!snapshot.is_ready_for(&["persistence"]));
    assert!(snapshot.is_ready_for(&["scoring"]));
}

#[test]
fn optional_dependency_outage_does_not_block_unrelated_work() {
    let snapshot = healthy_snapshot(vec![
        CapabilityHealth::new("authenticated_linking", CapabilityState::Unavailable, false).unwrap(),
        CapabilityHealth::new("scoring", CapabilityState::Available, true).unwrap(),
    ]);

    assert!(snapshot.is_ready_for(&[]));
    assert!(snapshot.is_ready_for(&["scoring"]));
    assert!(!snapshot.is_ready_for(&["authenticated_linking"]));
}

#[test]
fn degraded_capability_can_explicitly_remain_safe_for_new_work() {
    let snapshot = healthy_snapshot(vec![
        CapabilityHealth::new("research_registration", CapabilityState::Degraded, true).unwrap(),
    ]);

    assert!(snapshot.is_ready_for(&["research_registration"]));
    assert_eq!(
        snapshot.capability("research_registration").unwrap().state(),
        CapabilityState::Degraded
    );
}

#[test]
fn capability_state_alone_cannot_claim_new_work_is_safe() {
    let snapshot = healthy_snapshot(vec![
        CapabilityHealth::new("temporal_analysis", CapabilityState::Degraded, false).unwrap(),
    ]);

    assert!(!snapshot.is_ready_for(&["temporal_analysis"]));
}

#[test]
fn integrity_and_backlog_failures_block_state_changing_readiness() {
    let capability = CapabilityHealth::new("scoring", CapabilityState::Available, true).unwrap();

    for integrity in [DataIntegrityHealth::Unknown, DataIntegrityHealth::Incompatible] {
        let snapshot = RuntimeHealthSnapshot::new(
            true,
            BacklogHealth::WithinBounds,
            integrity,
            vec![capability.clone()],
        )
        .unwrap();
        assert!(!snapshot.is_ready_for(&["scoring"]));
    }

    for backlog in [BacklogHealth::Unknown, BacklogHealth::Stalled] {
        let snapshot = RuntimeHealthSnapshot::new(
            true,
            backlog,
            DataIntegrityHealth::Verified,
            vec![capability.clone()],
        )
        .unwrap();
        assert!(!snapshot.is_ready_for(&["scoring"]));
    }
}

#[test]
fn nonlive_process_and_unknown_required_capability_fail_closed() {
    let snapshot = RuntimeHealthSnapshot::new(
        false,
        BacklogHealth::WithinBounds,
        DataIntegrityHealth::Verified,
        vec![],
    )
    .unwrap();

    assert!(!snapshot.is_ready_for(&[]));
    assert!(!snapshot.is_ready_for(&["unregistered_capability"]));
}

#[test]
fn capability_identity_is_opaque_unique_and_queryable() {
    assert_eq!(
        CapabilityHealth::new("12345", CapabilityState::Available, true),
        Err(HealthContractError::InvalidReference)
    );

    let scoring = CapabilityHealth::new("scoring", CapabilityState::Available, true).unwrap();
    assert_eq!(scoring.capability_ref(), "scoring");
    assert!(scoring.accepts_new_work());

    assert_eq!(
        RuntimeHealthSnapshot::new(
            true,
            BacklogHealth::WithinBounds,
            DataIntegrityHealth::Verified,
            vec![scoring.clone(), scoring],
        ),
        Err(HealthContractError::DuplicateCapabilityReference)
    );
}

#[test]
fn health_contract_errors_have_stable_safe_display_text() {
    assert_eq!(
        HealthContractError::InvalidReference.to_string(),
        "health capability references must be opaque non-numeric values"
    );
    assert_eq!(
        HealthContractError::DuplicateCapabilityReference.to_string(),
        "health capability references must be unique within one snapshot"
    );
}
