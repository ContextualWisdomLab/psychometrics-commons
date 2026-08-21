//! Concurrency contract for append-only anonymous credential revocation evidence.
//!
//! Two identical revocation attempts may race after both transactions observed the same
//! unrevoked credential. The committed durable outcome is still one revocation, and the loser
//! must classify the identical committed evidence as an idempotent duplicate rather than as a
//! conflicting revocation.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;
use psychometrics_commons_runtime::postgres_anonymous_credential::{
    apply_anonymous_credential_migration, load_anonymous_credential, persist_anonymous_credential,
    AnonymousCredentialPersistenceDisposition,
};
use std::thread;
use std::time::Duration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA: &str = "anonymous_credential_concurrency_test";

fn connect(application_name: &str) -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "SET application_name TO '{application_name}'; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    client
}

fn credential() -> AnonymousCredential {
    AnonymousCredential::new(
        "anonymous_credential_concurrent",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        DIGEST,
        1_000,
        2_000,
    )
    .unwrap()
}

fn persist_initial(client: &mut Client, credential: &AnonymousCredential) {
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_anonymous_credential(&mut transaction, credential).unwrap(),
        AnonymousCredentialPersistenceDisposition::Inserted
    );
    transaction.commit().unwrap();
}

fn revoke_worker(
    application_name: &'static str,
) -> thread::JoinHandle<Result<AnonymousCredentialPersistenceDisposition, String>> {
    thread::spawn(move || {
        let mut client = connect(application_name);
        let mut revoked = credential();
        revoked.revoke(1_500).unwrap();
        let mut transaction = client.transaction().unwrap();
        match persist_anonymous_credential(&mut transaction, &revoked) {
            Ok(disposition) => {
                transaction.commit().unwrap();
                Ok(disposition)
            }
            Err(error) => {
                transaction.rollback().unwrap();
                Err(error.to_string())
            }
        }
    })
}

#[test]
fn concurrent_identical_revocations_are_one_revoke_plus_one_duplicate() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut setup = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    setup
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_anonymous_credential_migration(&mut setup).unwrap();
    persist_initial(&mut setup, &credential());

    // Hold the durable row so both workers can independently observe the same unrevoked snapshot
    // and then block at the append-only UPDATE. This makes the lost-update classification race
    // deterministic instead of relying on thread timing.
    let mut blocker = connect("anonymous_revoke_blocker");
    let mut blocker_transaction = blocker.transaction().unwrap();
    blocker_transaction
        .query_one(
            "SELECT credential_ref FROM anonymous_credential_evidence \
             WHERE credential_ref = $1 FOR UPDATE",
            &[&"anonymous_credential_concurrent"],
        )
        .unwrap();

    let first = revoke_worker("anonymous_revoke_worker_first");
    let second = revoke_worker("anonymous_revoke_worker_second");

    let mut both_waiting = false;
    for _ in 0..200 {
        let waiting: i64 = setup
            .query_one(
                "SELECT count(*) FROM pg_stat_activity \
                 WHERE application_name IN ('anonymous_revoke_worker_first', 'anonymous_revoke_worker_second') \
                   AND wait_event_type = 'Lock'",
                &[],
            )
            .unwrap()
            .get(0);
        if waiting == 2 {
            both_waiting = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        both_waiting,
        "both revoke transactions must reach the blocked update before the row lock is released"
    );
    blocker_transaction.commit().unwrap();

    let results = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Ok(AnonymousCredentialPersistenceDisposition::Revoked)
            ))
            .count(),
        1,
        "exactly one racer must append the revocation"
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(AnonymousCredentialPersistenceDisposition::Duplicate)))
            .count(),
        1,
        "the identical loser must reclassify the committed revocation as an idempotent duplicate: {results:?}"
    );

    let loaded = load_anonymous_credential(&mut setup, "anonymous_credential_concurrent")
        .unwrap()
        .expect("the raced credential must remain durable");
    assert_eq!(loaded.revoked_at_unix_ms(), Some(1_500));
}
