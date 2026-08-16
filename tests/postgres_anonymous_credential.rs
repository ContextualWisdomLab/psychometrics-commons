//! PostgreSQL contract tests for short-lived anonymous assessment credentials.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;
use psychometrics_commons_runtime::postgres_anonymous_credential::{
    apply_anonymous_credential_migration, load_anonymous_credential,
    persist_anonymous_credential_issue, persist_anonymous_credential_revocation,
    AnonymousCredentialPersistenceDisposition, AnonymousCredentialPersistenceError,
    AnonymousCredentialRevocationDisposition,
};

const PROOF_ALPHA: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROOF_BETA: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PROOF_GAMMA: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn client(schema_prefix: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_anonymous_credential_migration(&mut client).unwrap();
    apply_anonymous_credential_migration(&mut client).unwrap();
    client
}

fn credential(
    credential_ref: &str,
    tenant_ref: &str,
    participant_ref: &str,
    session_ref: &str,
    proof_digest: &str,
) -> AnonymousCredential {
    AnonymousCredential::new(
        credential_ref,
        tenant_ref,
        participant_ref,
        session_ref,
        proof_digest,
        10_000,
        20_000,
    )
    .unwrap()
}

#[test]
fn issue_load_and_exact_replay_preserve_digest_and_binding() {
    let mut client = client("anonymous_credential_issue");
    let credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_credential_issue(&mut transaction, &credential).unwrap(),
            AnonymousCredentialPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_credential_issue(&mut transaction, &credential).unwrap(),
            AnonymousCredentialPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    let loaded = load_anonymous_credential(&mut transaction, "credential_alpha", "tenant_alpha")
        .unwrap()
        .expect("credential must reload after restart");
    assert_eq!(loaded, credential);
    assert!(loaded.authorizes(
        PROOF_ALPHA,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        15_000,
    ));
    assert!(!loaded.authorizes(
        PROOF_ALPHA,
        "tenant_alpha",
        "participant_alpha",
        "session_beta",
        15_000,
    ));
    assert!(load_anonymous_credential(
        &mut transaction,
        "credential_alpha",
        "tenant_beta"
    )
    .unwrap()
    .is_none());
    transaction.rollback().unwrap();
}

#[test]
fn issue_replay_fails_closed_on_rebinding_or_digest_reuse() {
    let mut client = client("anonymous_credential_conflict");
    let credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_issue(&mut transaction, &credential).unwrap();
        transaction.commit().unwrap();
    }

    let rebound = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_beta",
        PROOF_ALPHA,
    );
    let reused_digest = credential(
        "credential_beta",
        "tenant_alpha",
        "participant_beta",
        "session_beta",
        PROOF_ALPHA,
    );

    for conflicting in [&rebound, &reused_digest] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_anonymous_credential_issue(&mut transaction, conflicting),
            Err(AnonymousCredentialPersistenceError::ConflictingReplay)
        ));
        transaction.rollback().unwrap();
    }
}

#[test]
fn issue_replay_after_revocation_is_idempotent_without_restoring_authority() {
    let mut client = client("anonymous_credential_issue_after_revoke");
    let issued = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    let mut revoked = issued.clone();
    revoked.revoke(15_000).unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_issue(&mut transaction, &issued).unwrap();
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_revocation(&mut transaction, &revoked).unwrap();
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_credential_issue(&mut transaction, &issued).unwrap(),
            AnonymousCredentialPersistenceDisposition::Duplicate
        );
        assert!(matches!(
            persist_anonymous_credential_issue(&mut transaction, &revoked),
            Err(AnonymousCredentialPersistenceError::InvalidCredentialState)
        ));
        transaction.rollback().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    let loaded = load_anonymous_credential(&mut transaction, "credential_alpha", "tenant_alpha")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.revoked_at_unix_ms(), Some(15_000));
    assert!(!loaded.authorizes(
        PROOF_ALPHA,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        15_000,
    ));
    transaction.rollback().unwrap();
}

#[test]
fn revocation_is_append_only_idempotent_and_reloadable() {
    let mut client = client("anonymous_credential_revoke");
    let mut credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_issue(&mut transaction, &credential).unwrap();
        transaction.commit().unwrap();
    }

    credential.revoke(15_000).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_credential_revocation(&mut transaction, &credential).unwrap(),
            AnonymousCredentialRevocationDisposition::Revoked
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_credential_revocation(&mut transaction, &credential).unwrap(),
            AnonymousCredentialRevocationDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    let loaded = load_anonymous_credential(&mut transaction, "credential_alpha", "tenant_alpha")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.revoked_at_unix_ms(), Some(15_000));
    assert!(loaded.authorizes(
        PROOF_ALPHA,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        14_999,
    ));
    assert!(!loaded.authorizes(
        PROOF_ALPHA,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        15_000,
    ));
    transaction.rollback().unwrap();
}

#[test]
fn revocation_rejects_missing_issue_rebinding_conflicting_time_and_unrevoked_input() {
    let mut client = client("anonymous_credential_revoke_conflict");
    let original = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );

    let mut missing_revoked = original.clone();
    missing_revoked.revoke(15_000).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_anonymous_credential_revocation(&mut transaction, &missing_revoked),
            Err(AnonymousCredentialPersistenceError::MissingCredential)
        ));
        transaction.rollback().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_anonymous_credential_revocation(&mut transaction, &original),
            Err(AnonymousCredentialPersistenceError::InvalidCredentialState)
        ));
        transaction.rollback().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_issue(&mut transaction, &original).unwrap();
        transaction.commit().unwrap();
    }

    let mut rebound = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_beta",
        "session_alpha",
        PROOF_ALPHA,
    );
    rebound.revoke(15_000).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_anonymous_credential_revocation(&mut transaction, &rebound),
            Err(AnonymousCredentialPersistenceError::ConflictingReplay)
        ));
        transaction.rollback().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_revocation(&mut transaction, &missing_revoked).unwrap();
        transaction.commit().unwrap();
    }

    let mut conflicting = original;
    conflicting.revoke(16_000).unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_anonymous_credential_revocation(&mut transaction, &conflicting),
        Err(AnonymousCredentialPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn durable_rows_reject_direct_rebinding_rewrite_delete_and_truncate() {
    let mut client = client("anonymous_credential_immutable");
    let credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_credential_issue(&mut transaction, &credential).unwrap();
        transaction.commit().unwrap();
    }

    assert!(client
        .execute(
            "UPDATE anonymous_session_credential SET session_ref = 'session_beta' WHERE credential_ref = 'credential_alpha'",
            &[],
        )
        .is_err());
    assert!(client
        .execute(
            "UPDATE anonymous_session_credential SET proof_digest = $1 WHERE credential_ref = 'credential_alpha'",
            &[&PROOF_BETA],
        )
        .is_err());
    assert!(client
        .execute(
            "UPDATE anonymous_session_credential SET revoked_at_unix_ms = 15000 WHERE credential_ref = 'credential_alpha'",
            &[],
        )
        .is_ok(), "the one allowed semantic mutation is first revocation evidence");
    assert!(client
        .execute(
            "UPDATE anonymous_session_credential SET revoked_at_unix_ms = 16000 WHERE credential_ref = 'credential_alpha'",
            &[],
        )
        .is_err());
    assert!(client
        .execute(
            "DELETE FROM anonymous_session_credential WHERE credential_ref = 'credential_alpha'",
            &[],
        )
        .is_err());
    assert!(client
        .batch_execute("TRUNCATE TABLE anonymous_session_credential")
        .is_err());
}

#[test]
fn persistence_fails_closed_for_unrepresentable_timestamps_and_corrupt_rows() {
    let mut client = client("anonymous_credential_corrupt");
    let overflow = AnonymousCredential::new(
        "credential_overflow",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
        i64::MAX as u64,
        (i64::MAX as u64) + 1,
    )
    .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_anonymous_credential_issue(&mut transaction, &overflow),
            Err(AnonymousCredentialPersistenceError::ValueOutOfRange)
        ));
        transaction.rollback().unwrap();
    }

    let mut revocation_overflow = credential(
        "credential_revocation_overflow",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_GAMMA,
    );
    revocation_overflow.revoke((i64::MAX as u64) + 1).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_anonymous_credential_revocation(&mut transaction, &revocation_overflow),
            Err(AnonymousCredentialPersistenceError::ValueOutOfRange)
        ));
        transaction.rollback().unwrap();
    }

    client
        .batch_execute(
            "ALTER TABLE anonymous_session_credential DISABLE TRIGGER USER;
             ALTER TABLE anonymous_session_credential DROP CONSTRAINT anonymous_credential_revocation_time_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO anonymous_session_credential
                (credential_ref, tenant_ref, participant_ref, session_ref, proof_digest,
                 issued_at_unix_ms, expires_at_unix_ms, revoked_at_unix_ms)
             VALUES
                ('credential_corrupt', 'tenant_alpha', 'participant_alpha', 'session_alpha', $1,
                 10000, 20000, 9000)",
            &[&PROOF_BETA],
        )
        .unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_anonymous_credential(&mut transaction, "credential_corrupt", "tenant_alpha"),
            Err(AnonymousCredentialPersistenceError::InconsistentEvidence)
        ));
        transaction.rollback().unwrap();
    }

    client
        .batch_execute(
            "ALTER TABLE anonymous_session_credential DROP CONSTRAINT anonymous_credential_issue_time_check;
             ALTER TABLE anonymous_session_credential DROP CONSTRAINT anonymous_credential_expiry_time_check;
             INSERT INTO anonymous_session_credential
                (credential_ref, tenant_ref, participant_ref, session_ref, proof_digest,
                 issued_at_unix_ms, expires_at_unix_ms, revoked_at_unix_ms)
             VALUES
                ('credential_negative', 'tenant_alpha', 'participant_alpha', 'session_alpha',
                 'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
                 -1, 20000, NULL);",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_anonymous_credential(&mut transaction, "credential_negative", "tenant_alpha"),
        Err(AnonymousCredentialPersistenceError::InconsistentEvidence)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn lookup_is_strict_and_serializable_transactions_are_rejected() {
    let mut client = client("anonymous_credential_fail_closed");
    {
        let mut transaction = client.transaction().unwrap();
        let error = load_anonymous_credential(&mut transaction, "1", "tenant_alpha").unwrap_err();
        assert!(matches!(
            &error,
            AnonymousCredentialPersistenceError::InvalidReference
        ));
        assert!(std::error::Error::source(&error).is_none());
        transaction.rollback().unwrap();
    }

    let credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_anonymous_credential_issue(&mut transaction, &credential),
        Err(AnonymousCredentialPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn database_failure_preserves_typed_source_and_safe_text() {
    let mut client = client("anonymous_credential_database_error");
    client
        .batch_execute("DROP TABLE anonymous_session_credential CASCADE")
        .unwrap();
    let credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    let mut transaction = client.transaction().unwrap();
    let error = persist_anonymous_credential_issue(&mut transaction, &credential).unwrap_err();
    assert!(matches!(
        &error,
        AnonymousCredentialPersistenceError::Database(_)
    ));
    assert_eq!(
        error.to_string(),
        "PostgreSQL anonymous credential persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
    transaction.rollback().unwrap();
}

#[test]
fn unexpected_insert_suppression_is_not_misclassified_as_duplicate() {
    let mut client = client("anonymous_credential_insert_suppression");
    client
        .batch_execute(
            "CREATE FUNCTION suppress_anonymous_credential_insert()
             RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END; $$;
             CREATE TRIGGER anonymous_credential_insert_suppression
             BEFORE INSERT ON anonymous_session_credential
             FOR EACH ROW EXECUTE FUNCTION suppress_anonymous_credential_insert();",
        )
        .unwrap();
    let credential = credential(
        "credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        PROOF_ALPHA,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_anonymous_credential_issue(&mut transaction, &credential),
        Err(AnonymousCredentialPersistenceError::InconsistentEvidence)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn public_errors_have_stable_non_sensitive_messages() {
    let cases = [
        (
            AnonymousCredentialPersistenceError::InvalidReference,
            "anonymous credential persistence references must be exact opaque values",
        ),
        (
            AnonymousCredentialPersistenceError::InvalidCredentialState,
            "anonymous credential persistence received the wrong lifecycle state",
        ),
        (
            AnonymousCredentialPersistenceError::MissingCredential,
            "anonymous credential revocation requires an existing tenant-bound issue record",
        ),
        (
            AnonymousCredentialPersistenceError::ConflictingReplay,
            "anonymous credential identity was replayed with conflicting evidence",
        ),
        (
            AnonymousCredentialPersistenceError::ValueOutOfRange,
            "anonymous credential timestamp exceeds the PostgreSQL bigint range",
        ),
        (
            AnonymousCredentialPersistenceError::InconsistentEvidence,
            "durable anonymous credential evidence cannot reconstruct a valid credential",
        ),
        (
            AnonymousCredentialPersistenceError::UnsupportedIsolationLevel,
            "anonymous credential persistence requires read committed isolation",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
