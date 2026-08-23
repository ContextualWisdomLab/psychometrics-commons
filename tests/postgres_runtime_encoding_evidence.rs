//! Fail-closed contract for `PostgreSQL` readiness when encoding evidence is absent.

use psychometrics_commons_runtime::health::CapabilityState;
use psychometrics_commons_runtime::postgres_health::{
    classify_postgres_runtime, PostgresRuntimeStatus,
};

#[test]
fn classifier_without_encoding_evidence_never_claims_write_readiness() {
    let health = classify_postgres_runtime(180_002, false);

    assert_ne!(health.status(), PostgresRuntimeStatus::Ready);
    assert_eq!(health.capability_state(), CapabilityState::Unavailable);
    assert!(!health.accepts_new_work());
}
