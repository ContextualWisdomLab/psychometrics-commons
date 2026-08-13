use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

#[test]
fn request_reference_must_remain_opaque_in_storage() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut db = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("data_rights_identity_{}", std::process::id());
    db.batch_execute(&format!("CREATE SCHEMA {schema}; SET search_path TO {schema};"))
        .unwrap();
    apply_integration_migration(&mut db).unwrap();
    apply_data_rights_migration(&mut db).unwrap();

    let error = db
        .execute(
            "INSERT INTO data_rights_request_state (request_ref, tenant_ref, participant_ref, request_kind, scope_ref, current_state, requested_at_unix_ms, latest_event_at_unix_ms) VALUES ('123', 'tenant_alpha', 'participant_alpha', 'export', 'scope_alpha', 'requested', 10000, 10000)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        error
            .as_db_error()
            .and_then(postgres::error::DbError::constraint),
        Some("data_rights_request_ref_format_check")
    );
}
