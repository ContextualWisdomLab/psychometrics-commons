//! Real `PostgreSQL` contract for short-lived anonymous credential evidence.
//!
//! A buyer can start an anonymous assessment only when the hashed proof survives
//! process restart and still authorizes the exact tenant, participant, and session.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;
use psychometrics_commons_runtime::postgres_anonymous_credential::{
    apply_anonymous_credential_migration, load_anonymous_credential,
    load_anonymous_credential_for_binding, persist_anonymous_credential,
    AnonymousCredentialPersistenceDisposition, AnonymousCredentialPersistenceError,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

static CREDENTIAL_TEST_LOCK: Mutex<()> = Mutex::new(());

fn credential_test_guard() -> MutexGuard<'static, ()> {
    CREDENTIAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS anonymous_credential_persistence_test;\
             SET search_path TO anonymous_credential_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_credential_table(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS anonymous_credential_persistence_test.anonymous_credential_evidence;",
        )
        .unwrap();
}

fn credential_named(credential_ref: &str, proof_digest: &str) -> AnonymousCredential {
    AnonymousCredential::new(
        credential_ref,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        proof_digest,
        1_000,
        2_000,
    )
    .unwrap()
}

fn persist_ok(
    client: &mut Client,
    credential: &AnonymousCredential,
) -> AnonymousCredentialPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_anonymous_credential(&mut transaction, credential).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    credential: &AnonymousCredential,
) -> AnonymousCredentialPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_anonymous_credential(&mut transaction, credential).unwrap_err();
    transaction.rollback().unwrap();
    error
}

#[test]
fn persisted_credential_reloads_and_authorizes_only_inside_the_server_window() {
    let _guard = credential_test_guard();
    let mut client = test_client();
    reset_credential_table(&mut client);
    apply_anonymous_credential_migration(&mut client).unwrap();

    let credential = credential_named("anonymous_credential_alpha", DIGEST_A);
    assert_eq!(
        persist_ok(&mut client, &credential),
        AnonymousCredentialPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &credential),
        AnonymousCredentialPersistenceDisposition::Duplicate
    );

    let loaded = load_anonymous_credential(&mut client, "anonymous_credential_alpha")
        .unwrap()
        .expect("issued credential evidence must survive restart");
    assert_eq!(loaded, credential);
    assert!(loaded.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));
    assert!(!loaded.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        2_000,
    ));

    let bound = load_anonymous_credential_for_binding(
        &mut client,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        DIGEST_A,
    )
    .unwrap()
    .expect("presented digest must recover the exact bound credential");
    assert_eq!(bound, credential);
    assert!(load_anonymous_credential_for_binding(
        &mut client,
        "tenant_other",
        "participant_alpha",
        "session_alpha",
        DIGEST_A,
    )
    .unwrap()
    .is_none());
    assert!(
        load_anonymous_credential(&mut client, "anonymous_credential_missing")
            .unwrap()
            .is_none()
    );
}

#[test]
fn credential_identity_and_digest_rebinding_fail_closed() {
    let _guard = credential_test_guard();
    let mut client = test_client();
    reset_credential_table(&mut client);
    apply_anonymous_credential_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &credential_named("anonymous_credential_alpha", DIGEST_A),
    );

    assert!(matches!(
        persist_err(
            &mut client,
            &credential_named("anonymous_credential_alpha", DIGEST_B),
        ),
        AnonymousCredentialPersistenceError::ConflictingReplay
    ));
    assert!(matches!(
        persist_err(
            &mut client,
            &credential_named("anonymous_credential_beta", DIGEST_A),
        ),
        AnonymousCredentialPersistenceError::ConflictingReplay
    ));
}

#[test]
fn revocation_is_append_only_across_restart() {
    let _guard = credential_test_guard();
    let mut client = test_client();
    reset_credential_table(&mut client);
    apply_anonymous_credential_migration(&mut client).unwrap();

    let mut credential = credential_named("anonymous_credential_alpha", DIGEST_A);
    persist_ok(&mut client, &credential);
    credential.revoke(1_500).unwrap();
    assert_eq!(
        persist_ok(&mut client, &credential),
        AnonymousCredentialPersistenceDisposition::Revoked
    );
    assert_eq!(
        persist_ok(&mut client, &credential),
        AnonymousCredentialPersistenceDisposition::Duplicate
    );

    let loaded = load_anonymous_credential(&mut client, "anonymous_credential_alpha")
        .unwrap()
        .expect("revocation evidence must survive restart");
    assert_eq!(loaded.revoked_at_unix_ms(), Some(1_500));
    assert!(loaded.is_valid_at(1_499));
    assert!(!loaded.is_valid_at(1_500));
    assert!(!loaded.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));

    let mut conflicting = credential_named("anonymous_credential_alpha", DIGEST_A);
    conflicting.revoke(1_501).unwrap();
    assert!(matches!(
        persist_err(&mut client, &conflicting),
        AnonymousCredentialPersistenceError::ConflictingRevocation
    ));
    assert!(matches!(
        persist_err(
            &mut client,
            &credential_named("anonymous_credential_alpha", DIGEST_A),
        ),
        AnonymousCredentialPersistenceError::ConflictingReplay
    ));
}

#[test]
fn persistence_requires_read_committed_and_exposes_stable_errors() {
    let _guard = credential_test_guard();
    let mut client = test_client();
    reset_credential_table(&mut client);
    apply_anonymous_credential_migration(&mut client).unwrap();

    let credential = credential_named("anonymous_credential_serializable", DIGEST_A);
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_anonymous_credential(&mut transaction, &credential),
        Err(AnonymousCredentialPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    for (error, expected) in [
        (
            AnonymousCredentialPersistenceError::InvalidReference,
            "anonymous credential persistence references must be exact canonical opaque values",
        ),
        (
            AnonymousCredentialPersistenceError::InvalidDigest,
            "anonymous credential persistence digest must be canonical lowercase SHA-256 evidence",
        ),
        (
            AnonymousCredentialPersistenceError::InvalidTimestamp,
            "anonymous credential persistence timestamp exceeds the PostgreSQL bigint range",
        ),
        (
            AnonymousCredentialPersistenceError::InvalidStoredEvidence,
            "anonymous credential stored evidence violated the domain contract",
        ),
        (
            AnonymousCredentialPersistenceError::ConflictingReplay,
            "anonymous credential identity was replayed with conflicting evidence",
        ),
        (
            AnonymousCredentialPersistenceError::ConflictingRevocation,
            "anonymous credential revocation evidence cannot be replaced",
        ),
        (
            AnonymousCredentialPersistenceError::UnsupportedIsolationLevel,
            "anonymous credential persistence requires read committed isolation",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
        assert!(Error::source(&error).is_none());
    }

    let database_error = client
        .query_one(
            "SELECT * FROM anonymous_credential_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = AnonymousCredentialPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL anonymous-credential persistence failed"
    );
    assert!(Error::source(&error).is_some());
}

#[test]
fn binding_load_rejects_padded_digest_aliases() {
    let _guard = credential_test_guard();
    let mut client = test_client();
    reset_credential_table(&mut client);
    apply_anonymous_credential_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &credential_named("anonymous_credential_alpha", DIGEST_A),
    );

    assert!(matches!(
        load_anonymous_credential_for_binding(
            &mut client,
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            " sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        Err(AnonymousCredentialPersistenceError::InvalidDigest)
    ));
    assert!(matches!(
        load_anonymous_credential(&mut client, " anonymous_credential_alpha "),
        Err(AnonymousCredentialPersistenceError::InvalidReference)
    ));
}

#[test]
fn already_revoked_first_insert_reloads_as_revoked() {
    let _guard = credential_test_guard();
    let mut client = test_client();
    reset_credential_table(&mut client);
    apply_anonymous_credential_migration(&mut client).unwrap();

    let mut credential = credential_named("anonymous_credential_alpha", DIGEST_A);
    credential.revoke(1_200).unwrap();
    assert_eq!(
        persist_ok(&mut client, &credential),
        AnonymousCredentialPersistenceDisposition::Inserted
    );
    let loaded = load_anonymous_credential(&mut client, "anonymous_credential_alpha")
        .unwrap()
        .expect("first insert may already carry revocation evidence");
    assert_eq!(loaded, credential);
    assert_eq!(
        persist_ok(&mut client, &credential),
        AnonymousCredentialPersistenceDisposition::Duplicate
    );
}
