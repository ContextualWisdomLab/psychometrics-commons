//! Deployment-policy retention contract for otherwise append-only audit evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_retention_test CASCADE;\
             CREATE SCHEMA audit_retention_test;\
             SET search_path TO audit_retention_test;",
        )
        .unwrap();
    client
}

fn insert_at(client: &mut Client, event_ref: &str, occurred_at_unix_ms: i64) {
    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &event_ref,
                &"tenant_research_alpha",
                &"actor_publisher_alpha",
                &"audit_retention",
                &"retain_or_expire_audit_evidence",
                &"audit_retention_policy_alpha",
                &"succeeded",
                &DIGEST,
                &occurred_at_unix_ms,
            ],
        )
        .unwrap();
}

#[test]
fn retention_execution_is_not_publicly_granted_and_direct_delete_stays_blocked() {
    let mut client = client();
    apply_audit_evidence_migration(&mut client).unwrap();

    let public_execute: bool = client
        .query_one(
            "SELECT EXISTS (\
                 SELECT 1 FROM information_schema.routine_privileges\
                 WHERE routine_schema = current_schema()\
                   AND routine_name = 'expire_audit_evidence_before'\
                   AND grantee = 'PUBLIC'\
                   AND privilege_type = 'EXECUTE'\
             )",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        !public_execute,
        "retention execution must require an explicit deployment grant"
    );

    insert_at(&mut client, "audit_event_direct_delete_01", 1_000);
    let direct_delete = client.execute(
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_direct_delete_01'",
        &[],
    );
    assert!(
        direct_delete.is_err(),
        "ordinary direct deletion must remain blocked even for a row eligible under some future policy"
    );
}

#[test]
fn explicit_retention_execution_deletes_only_rows_strictly_before_the_cutoff() {
    let mut client = client();
    apply_audit_evidence_migration(&mut client).unwrap();

    insert_at(&mut client, "audit_event_old_01", 1_000);
    insert_at(&mut client, "audit_event_boundary_01", 2_000);
    insert_at(&mut client, "audit_event_new_01", 3_000);

    let deleted: i64 = client
        .query_one("SELECT expire_audit_evidence_before($1)", &[&2_000_i64])
        .unwrap()
        .get(0);
    assert_eq!(deleted, 1);

    let remaining: Vec<String> = client
        .query(
            "SELECT audit_event_ref FROM audit_evidence_record ORDER BY occurred_at_unix_ms",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        remaining,
        vec!["audit_event_boundary_01", "audit_event_new_01"],
        "retention must use an explicit exclusive cutoff and preserve boundary/newer evidence"
    );
}

#[test]
fn retention_rejects_zero_and_future_cutoffs_instead_of_inventing_policy() {
    let mut client = client();
    apply_audit_evidence_migration(&mut client).unwrap();

    assert!(client
        .query_one("SELECT expire_audit_evidence_before(0)", &[])
        .is_err());
    assert!(client
        .query_one(
            "SELECT expire_audit_evidence_before((extract(epoch FROM transaction_timestamp()) * 1000)::bigint + 60000)",
            &[],
        )
        .is_err());
}
