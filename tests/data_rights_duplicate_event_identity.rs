//! Shared outbox event identity cannot fan out to two dependent systems.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    persist_requested_data_rights_with_propagation, DataRightsPersistenceError,
    DataRightsPropagationTarget,
};

#[test]
fn duplicate_propagation_event_identity_is_rejected_before_database_work() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let mut db = Client::connect(&url, NoTls).unwrap();
    let request = DataRightsRequest::new(
        "request_duplicate_event",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        1,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "event_shared",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "request_duplicate_event",
        1,
        "request_duplicate_event",
        None,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    let targets = [
        DataRightsPropagationTarget::new("system_alpha", &event),
        DataRightsPropagationTarget::new("system_beta", &event),
    ];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3),
        Err(DataRightsPersistenceError::DuplicateEventIdentity)
    ));
}
