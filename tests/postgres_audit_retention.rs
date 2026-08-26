//! Deployment-policy retention contract for otherwise append-only audit evidence.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;
use psychometrics_commons_runtime::postgres_audit_retention::apply_audit_evidence_retention_migration;

const AUDIT_RETENTION_TEST_LOCK_KEY: i64 = 0x4155_4454_5245_544e;
const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn acquire_retention_fixture_guard(
    mut guard: Client,
    lock_timeout: &str,
) -> Result<Client, postgres::Error> {
    guard.query_one(
        "SELECT set_config('lock_timeout', $1, false)",
        &[&lock_timeout],
    )?;
    guard.query_one(
        "SELECT pg_advisory_lock($1)",
        &[&AUDIT_RETENTION_TEST_LOCK_KEY],
    )?;
    Ok(guard)
}

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let mut client = acquire_retention_fixture_guard(guard, "60s")
        .expect("shared audit-retention fixture lock must be acquired within 60 seconds");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_retention_test CASCADE;\
             CREATE SCHEMA audit_retention_test;\
             SET search_path TO audit_retention_test;",
        )
        .unwrap();
    client
}

fn apply_audit_migrations(client: &mut Client) {
    apply_audit_evidence_migration(client).unwrap();
    apply_audit_evidence_retention_migration(client).unwrap();
}

fn insert_at(
    client: &mut Client,
    tenant_ref: &str,
    event_ref: &str,
    occurred_at_unix_ms: i64,
) {
    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &event_ref,
                &tenant_ref,
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
fn retention_fixture_guard_is_visible_to_another_postgres_session() {
    let _guard = client();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&AUDIT_RETENTION_TEST_LOCK_KEY],
        )
        .expect("contender advisory-lock probe must execute")
        .get(0);

    assert!(
        !acquired,
        "fixed-schema retention fixtures must serialize across PostgreSQL sessions"
    );
}

#[test]
fn retention_fixture_lock_contention_has_a_finite_timeout() {
    let _guard = client();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let error = acquire_retention_fixture_guard(contender, "100ms")
        .err()
        .expect("contended fixture acquisition must time out instead of waiting indefinitely");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));
}

#[test]
fn retention_execution_is_not_publicly_granted_and_direct_delete_stays_blocked() {
    let mut client = client();
    apply_audit_migrations(&mut client);

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

    insert_at(
        &mut client,
        "tenant_research_alpha",
        "audit_event_direct_delete_01",
        1_000,
    );
    let direct_delete = client.execute(
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_direct_delete_01'",
        &[],
    );
    assert!(
        direct_delete.is_err(),
        "ordinary direct deletion must remain blocked even for a row eligible under some future policy"
    );

    let mut bypass = client
        .transaction()
        .expect("direct-GUC bypass probe must start a transaction");
    bypass
        .query_one(
            "SELECT set_config('psychometrics.audit_retention_execution', 'on', true)",
            &[],
        )
        .expect("owner-session bypass probe must be able to set the caller-settable GUC");
    let guc_delete = bypass.execute(
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_direct_delete_01'",
        &[],
    );
    assert!(
        guc_delete.is_err(),
        "setting the retention GUC directly must not authorize deletion outside the bounded routine"
    );
}

#[test]
fn reapplying_core_audit_migration_preserves_bounded_retention_execution() {
    let mut client = client();
    apply_audit_migrations(&mut client);

    insert_at(
        &mut client,
        "tenant_research_alpha",
        "audit_event_reapply_old_01",
        1_000,
    );
    insert_at(
        &mut client,
        "tenant_research_alpha",
        "audit_event_reapply_boundary_01",
        2_000,
    );

    apply_audit_evidence_migration(&mut client).unwrap();

    let deleted: i64 = client
        .query_one(
            "SELECT expire_audit_evidence_before($1, $2)",
            &[&"tenant_research_alpha", &2_000_i64],
        )
        .expect("reapplying migration 0040 must not erase the migration 0041 retention guard")
        .get(0);
    assert_eq!(deleted, 1);

    let remaining: Vec<String> = client
        .query(
            "SELECT audit_event_ref FROM audit_evidence_record ORDER BY audit_event_ref",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(remaining, vec!["audit_event_reapply_boundary_01"]);
}

#[test]
fn explicit_retention_execution_is_tenant_scoped_and_exclusive_at_the_cutoff() {
    let mut client = client();
    apply_audit_migrations(&mut client);

    insert_at(
        &mut client,
        "tenant_research_alpha",
        "audit_event_alpha_old_01",
        1_000,
    );
    insert_at(
        &mut client,
        "tenant_research_alpha",
        "audit_event_alpha_boundary_01",
        2_000,
    );
    insert_at(
        &mut client,
        "tenant_research_alpha",
        "audit_event_alpha_new_01",
        3_000,
    );
    insert_at(
        &mut client,
        "tenant_research_beta",
        "audit_event_beta_old_01",
        1_000,
    );

    let deleted: i64 = client
        .query_one(
            "SELECT expire_audit_evidence_before($1, $2)",
            &[&"tenant_research_alpha", &2_000_i64],
        )
        .unwrap()
        .get(0);
    assert_eq!(deleted, 1);

    let remaining: Vec<(String, String)> = client
        .query(
            "SELECT tenant_ref, audit_event_ref\
             FROM audit_evidence_record\
             ORDER BY tenant_ref, occurred_at_unix_ms, audit_event_ref",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(
        remaining,
        vec![
            (
                "tenant_research_alpha".to_owned(),
                "audit_event_alpha_boundary_01".to_owned(),
            ),
            (
                "tenant_research_alpha".to_owned(),
                "audit_event_alpha_new_01".to_owned(),
            ),
            (
                "tenant_research_beta".to_owned(),
                "audit_event_beta_old_01".to_owned(),
            ),
        ],
        "retention must preserve cutoff-boundary/newer evidence and every other tenant"
    );

    let post_retention_direct_delete = client.execute(
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_alpha_boundary_01'",
        &[],
    );
    assert!(
        post_retention_direct_delete.is_err(),
        "retention authority must be transaction-local to the function and cleared before return"
    );
}

#[test]
fn retention_rejects_invalid_tenant_zero_and_future_cutoffs_instead_of_inventing_policy() {
    let mut client = client();
    apply_audit_migrations(&mut client);

    for invalid_tenant in ["", " tenant_research_alpha ", "123", "tenant\u{200b}_alpha"] {
        assert!(client
            .query_one(
                "SELECT expire_audit_evidence_before($1, $2)",
                &[&invalid_tenant, &2_000_i64],
            )
            .is_err());
    }
    assert!(client
        .query_one(
            "SELECT expire_audit_evidence_before(NULL::text, 2000::bigint)",
            &[],
        )
        .is_err());
    assert!(client
        .query_one(
            "SELECT expire_audit_evidence_before('tenant_research_alpha', NULL::bigint)",
            &[],
        )
        .is_err());
    assert!(client
        .query_one(
            "SELECT expire_audit_evidence_before($1, 0)",
            &[&"tenant_research_alpha"],
        )
        .is_err());
    assert!(client
        .query_one(
            "SELECT expire_audit_evidence_before(\
                 $1,\
                 (extract(epoch FROM transaction_timestamp()) * 1000)::bigint + 60000\
             )",
            &[&"tenant_research_alpha"],
        )
        .is_err());
}