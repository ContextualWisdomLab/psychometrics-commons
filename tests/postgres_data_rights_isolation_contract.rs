use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_requested_data_rights_with_propagation,
    DataRightsPersistenceError, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

#[test]
fn serializable_session_default_is_rejected() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let mut db = Client::connect(&url, NoTls).unwrap();
    let schema = format!("data_rights_iso_{}", std::process::id());
    db.batch_execute(&format!("CREATE SCHEMA {schema}; SET search_path TO {schema};"))
        .unwrap();
    apply_integration_migration(&mut db).unwrap();
    apply_data_rights_migration(&mut db).unwrap();
    db.batch_execute("SET default_transaction_isolation TO 'serializable'")
        .unwrap();

    let request = DataRightsRequest::new(
        "data_rights_request_iso",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "data_rights_event_iso",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_iso",
        10_000,
        "data_rights_request_iso",
        None,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    let targets = [DataRightsPropagationTarget::new("dependent_system_alpha", &event)];

    let error = persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3)
        .unwrap_err();
    assert!(matches!(
        &error,
        DataRightsPersistenceError::UnsupportedIsolationLevel
    ));
    assert_eq!(
        error.to_string(),
        "data-rights persistence requires read committed isolation"
    );
}
