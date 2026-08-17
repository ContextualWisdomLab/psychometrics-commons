//! PostgreSQL reference-shape contracts for durable data-rights completion evidence.
//!
//! The Rust domain rejects Unicode-whitespace aliases and Unicode numeric-like references. The
//! physical completion schema must preserve the same boundary even when migration 0024 is
//! reapplied over an earlier, weaker revision of its named CHECK constraints.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_data_rights_completion::{
    apply_data_rights_completion_migration, persist_data_rights_completion,
};
use psychometrics_commons_runtime::postgres_data_rights_processing::{
    apply_data_rights_processing_migration, persist_data_rights_processing_start,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client(schema_prefix: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();
    client
}

fn persist_processing(client: &mut Client, request_ref: &str) -> DataRightsRequest {
    let mut request = DataRightsRequest::new(
        request_ref,
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        &format!("event_{request_ref}"),
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        request_ref,
        10_000,
        request_ref,
        None,
        DIGEST,
    )
    .unwrap();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(client, &request, &targets, 3).unwrap();
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_identity_verification(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }
    request.start_processing("operation_alpha", 10_200).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_processing_start(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }
    request
}

fn seed_partial_completion(schema_prefix: &str, request_ref: &str) -> Client {
    let mut client = test_client(schema_prefix);
    let mut request = persist_processing(&mut client, request_ref);
    apply_data_rights_completion_migration(&mut client).unwrap();

    // Simulate an earlier revision of migration 0024 whose named constraints used ASCII-oriented
    // btrim/default-collation checks. Reapplication must replace, not merely find, these names.
    client
        .batch_execute(
            "ALTER TABLE data_rights_request_state
                 DROP CONSTRAINT data_rights_completion_evidence_ref_format_check;
             ALTER TABLE data_rights_request_state
                 ADD CONSTRAINT data_rights_completion_evidence_ref_format_check CHECK (
                     completion_evidence_ref IS NULL
                     OR (
                         completion_evidence_ref = btrim(completion_evidence_ref)
                         AND completion_evidence_ref <> ''
                         AND NOT (
                             completion_evidence_ref ~ '[[:digit:]]'
                             AND completion_evidence_ref ~ '^[[:digit:]+,.eE-]+$'
                         )
                     )
                 );
             ALTER TABLE data_rights_retained_scope_evidence
                 DROP CONSTRAINT data_rights_retained_scope_ref_format_check;
             ALTER TABLE data_rights_retained_scope_evidence
                 ADD CONSTRAINT data_rights_retained_scope_ref_format_check CHECK (
                     retained_scope_ref = btrim(retained_scope_ref)
                     AND retained_scope_ref <> ''
                     AND NOT (
                         retained_scope_ref ~ '[[:digit:]]'
                         AND retained_scope_ref ~ '^[[:digit:]+,.eE-]+$'
                     )
                 );",
        )
        .unwrap();

    apply_data_rights_completion_migration(&mut client).unwrap();
    request
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_completion(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }
    client
}

#[test]
fn reapply_rejects_unicode_aliases_for_completion_evidence() {
    let mut client = seed_partial_completion(
        "data_rights_completion_ref_constraint",
        "data_rights_request_completion_ref",
    );

    for invalid_ref in [
        "U&'\\00A0completion_evidence_alpha'",
        "U&'12\\066B3'",
        "U&'12\\FF0E3'",
    ] {
        let statement = format!(
            "UPDATE data_rights_request_state
             SET completion_evidence_ref = {invalid_ref}
             WHERE request_ref = 'data_rights_request_completion_ref'"
        );
        assert!(
            client.batch_execute(&statement).is_err(),
            "completion evidence must preserve the domain's Unicode opaque-reference boundary: {invalid_ref}"
        );
    }
}

#[test]
fn reapply_rejects_unicode_aliases_for_retained_scope_evidence() {
    let mut client = seed_partial_completion(
        "data_rights_retained_scope_ref_constraint",
        "data_rights_request_retained_scope_ref",
    );

    for invalid_ref in [
        "U&'\\00A0retention_shadow'",
        "U&'12\\066B3'",
        "U&'12\\FF0E3'",
    ] {
        let statement = format!(
            "INSERT INTO data_rights_retained_scope_evidence
                 (request_ref, tenant_ref, retained_scope_ref)
             VALUES (
                 'data_rights_request_retained_scope_ref',
                 'tenant_alpha',
                 {invalid_ref}
             )"
        );
        assert!(
            client.batch_execute(&statement).is_err(),
            "retained-scope evidence must preserve the domain's Unicode opaque-reference boundary: {invalid_ref}"
        );
    }
}
