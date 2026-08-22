//! Durable inbox-consumption identity must match the Rust opaque-reference boundary.
//!
//! `integration_consumption` is downstream of `integration_inbox`, but it is independently
//! persisted lifecycle evidence. Direct SQL and migration replay must therefore reject the same
//! Unicode-numeric aliases, Unicode outer whitespace, and embedded controls as
//! `normalized_reference`.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_inbox_consumption::apply_inbox_consumption_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS integration_consumption_reference_parity_test CASCADE; \
             CREATE SCHEMA integration_consumption_reference_parity_test; \
             SET search_path TO integration_consumption_reference_parity_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_inbox_consumption_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_inbox(client: &mut Client, suffix: &str) {
    client
        .execute(
            "INSERT INTO integration_inbox (\
                 consumer_ref, source_ref, tenant_ref, source_event_ref, event_type, schema_version,\
                 subject_ref, payload_digest, received_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,'assessment.session.completed','v1',$5,$6,10000)",
            &[
                &format!("consumer_{suffix}"),
                &format!("source_{suffix}"),
                &format!("tenant_{suffix}"),
                &format!("event_{suffix}"),
                &format!("subject_{suffix}"),
                &DIGEST,
            ],
        )
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn insert_pending(
    client: &mut Client,
    consumer_ref: &str,
    source_ref: &str,
    tenant_ref: &str,
    source_event_ref: &str,
    consumption_ref: &str,
    side_effect_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO integration_consumption (\
             consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref,\
             side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms\
         ) VALUES ($1,$2,$3,$4,$5,$6,'pending',0,10000)",
        &[
            &consumer_ref,
            &source_ref,
            &tenant_ref,
            &source_event_ref,
            &consumption_ref,
            &side_effect_ref,
        ],
    )
}

#[derive(Clone, Copy, Debug)]
enum ConsumptionReferenceField {
    Consumer,
    Source,
    Tenant,
    SourceEvent,
    Consumption,
    SideEffect,
}

fn assert_required_field_rejects_invalid_aliases(
    client: &mut Client,
    field: ConsumptionReferenceField,
    constraint: &str,
) {
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let suffix = format!("{}_{}", field as u8, index);
        insert_inbox(client, &suffix);
        let mut consumer_ref = format!("consumer_{suffix}");
        let mut source_ref = format!("source_{suffix}");
        let mut tenant_ref = format!("tenant_{suffix}");
        let mut event_ref = format!("event_{suffix}");
        let mut consumption_ref = format!("consumption_{suffix}");
        let mut side_effect_ref = format!("side_effect_{suffix}");
        match field {
            ConsumptionReferenceField::Consumer => consumer_ref = invalid_ref.to_owned(),
            ConsumptionReferenceField::Source => source_ref = invalid_ref.to_owned(),
            ConsumptionReferenceField::Tenant => tenant_ref = invalid_ref.to_owned(),
            ConsumptionReferenceField::SourceEvent => event_ref = invalid_ref.to_owned(),
            ConsumptionReferenceField::Consumption => consumption_ref = invalid_ref.to_owned(),
            ConsumptionReferenceField::SideEffect => side_effect_ref = invalid_ref.to_owned(),
        }

        let error = insert_pending(
            client,
            &consumer_ref,
            &source_ref,
            &tenant_ref,
            &event_ref,
            &consumption_ref,
            &side_effect_ref,
        )
        .expect_err("direct SQL must not bypass the Rust consumption-reference boundary");
        assert_check(&error, constraint);
    }
}

#[test]
fn all_required_consumption_references_reject_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client();

    for (field, constraint) in [
        (
            ConsumptionReferenceField::Consumer,
            "integration_consumption_consumer_ref_check",
        ),
        (
            ConsumptionReferenceField::Source,
            "integration_consumption_source_ref_check",
        ),
        (
            ConsumptionReferenceField::Tenant,
            "integration_consumption_tenant_ref_check",
        ),
        (
            ConsumptionReferenceField::SourceEvent,
            "integration_consumption_source_event_ref_check",
        ),
        (
            ConsumptionReferenceField::Consumption,
            "integration_consumption_consumption_ref_check",
        ),
        (
            ConsumptionReferenceField::SideEffect,
            "integration_consumption_side_effect_ref_check",
        ),
    ] {
        assert_required_field_rejects_invalid_aliases(&mut client, field, constraint);
    }
}

#[test]
fn completion_evidence_and_quarantine_cause_use_the_same_reference_boundary() {
    let _guard = guard();
    let mut client = client();

    for (index, invalid_ref) in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}evidence_alpha",
        "evidence_\u{0001}_alpha",
    ]
    .into_iter()
    .enumerate()
    {
        let suffix = format!("completed_{index}");
        insert_inbox(&mut client, &suffix);
        let error = client
            .execute(
                "INSERT INTO integration_consumption (\
                     consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref,\
                     side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms,\
                     completion_evidence_ref\
                 ) VALUES ($1,$2,$3,$4,$5,$6,'completed',1,10000,$7)",
                &[
                    &format!("consumer_{suffix}"),
                    &format!("source_{suffix}"),
                    &format!("tenant_{suffix}"),
                    &format!("event_{suffix}"),
                    &format!("consumption_{suffix}"),
                    &format!("side_effect_{suffix}"),
                    &invalid_ref,
                ],
            )
            .expect_err("completion evidence must use the Rust reference boundary");
        assert_check(
            &error,
            "integration_consumption_completion_evidence_ref_check",
        );
    }

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}cause_alpha", "cause_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let suffix = format!("quarantined_{index}");
        insert_inbox(&mut client, &suffix);
        let error = client
            .execute(
                "INSERT INTO integration_consumption (\
                     consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref,\
                     side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms,\
                     cause_code\
                 ) VALUES ($1,$2,$3,$4,$5,$6,'quarantined',1,10000,$7)",
                &[
                    &format!("consumer_{suffix}"),
                    &format!("source_{suffix}"),
                    &format!("tenant_{suffix}"),
                    &format!("event_{suffix}"),
                    &format!("consumption_{suffix}"),
                    &format!("side_effect_{suffix}"),
                    &invalid_ref,
                ],
            )
            .expect_err("quarantine cause must use the Rust reference boundary");
        assert_check(&error, "integration_consumption_cause_code_check");
    }
}

#[test]
fn migration_reapplication_repairs_weakened_consumption_reference_constraint() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE integration_consumption \
                 DROP CONSTRAINT integration_consumption_consumption_ref_check; \
             ALTER TABLE integration_consumption \
                 ADD CONSTRAINT integration_consumption_consumption_ref_check CHECK (\
                     consumption_ref = btrim(consumption_ref) AND consumption_ref <> ''\
                 );",
        )
        .unwrap();

    apply_inbox_consumption_migration(&mut client).unwrap();

    insert_inbox(&mut client, "upgrade_guard");
    let error = insert_pending(
        &mut client,
        "consumer_upgrade_guard",
        "source_upgrade_guard",
        "tenant_upgrade_guard",
        "event_upgrade_guard",
        "½",
        "side_effect_upgrade_guard",
    )
    .expect_err("reapplication must restore the exact reference predicate");
    assert_check(&error, "integration_consumption_consumption_ref_check");
}

#[test]
fn migration_reapplication_fails_closed_on_historical_invalid_consumption_identity() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE integration_consumption \
                 DROP CONSTRAINT integration_consumption_consumption_ref_check; \
             ALTER TABLE integration_consumption \
                 ADD CONSTRAINT integration_consumption_consumption_ref_check CHECK (\
                     consumption_ref = btrim(consumption_ref) AND consumption_ref <> ''\
                 );",
        )
        .unwrap();
    insert_inbox(&mut client, "historical");
    insert_pending(
        &mut client,
        "consumer_historical",
        "source_historical",
        "tenant_historical",
        "event_historical",
        "½",
        "side_effect_historical",
    )
    .expect("weakened historical predicate must admit the regression fixture");

    let error = apply_inbox_consumption_migration(&mut client)
        .expect_err("upgrade must fail closed instead of blessing invalid durable identity");
    assert_check(&error, "integration_consumption_consumption_ref_check");
}
