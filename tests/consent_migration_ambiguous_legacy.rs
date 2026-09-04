//! Upgrade guard for legacy consent histories whose relative order cannot be proven.
//!
//! ADR-0015 requires a migration to fail rather than invent provenance when a new
//! immutable field cannot be deterministically backfilled. Two or more pre-ordering
//! consent events for one participant have no durable relative-order authority, so
//! migration 0021 must stop before adding `event_sequence` instead of installing a
//! schema that the new runtime cannot safely load.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_consent::apply_consent_migration;

const LEGACY_CONSENT_MIGRATION: &str = include_str!("../migrations/0005_consent_lifecycle.sql");

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn ambiguous_multi_event_legacy_history_blocks_order_migration_before_schema_change() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS consent_migration_ambiguous_test CASCADE;\
             CREATE SCHEMA consent_migration_ambiguous_test;\
             SET search_path TO consent_migration_ambiguous_test;",
        )
        .unwrap();
    client.batch_execute(LEGACY_CONSENT_MIGRATION).unwrap();

    client
        .execute(
            "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
            &[&"participant_consent_ambiguous_upgrade"],
        )
        .unwrap();
    for (event_ref, decision) in [
        ("consent_event_ambiguous_grant", "granted"),
        ("consent_event_ambiguous_revoke", "revoked"),
    ] {
        client
            .execute(
                "INSERT INTO consent_event (\
                     participant_ref, event_ref, consent_purpose, consent_decision,\
                     consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
                 ) VALUES ($1, $2, 'research_contribution', $3, $4, $5, 61000)",
                &[
                    &"participant_consent_ambiguous_upgrade",
                    &event_ref,
                    &decision,
                    &"consent_form_research_v1",
                    &"research_scope_research_v1",
                ],
            )
            .unwrap();
    }

    apply_consent_migration(&mut client)
        .expect_err("ambiguous legacy order must block the ordering migration");

    let event_sequence_exists: bool = client
        .query_one(
            "SELECT EXISTS ( \
                 SELECT 1 FROM information_schema.columns \
                 WHERE table_schema = 'consent_migration_ambiguous_test' \
                   AND table_name = 'consent_event' \
                   AND column_name = 'event_sequence' \
             )",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        !event_sequence_exists,
        "failed upgrade must leave the legacy schema intact for rollback"
    );

    let retained_event_count: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM consent_event WHERE participant_ref = $1",
            &[&"participant_consent_ambiguous_upgrade"],
        )
        .unwrap()
        .get(0);
    assert_eq!(retained_event_count, 2);
}
