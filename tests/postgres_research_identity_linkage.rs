//! Real `PostgreSQL` contract for restricted research-identity linkage.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_research_identity_linkage::{
    apply_research_identity_linkage_migration, load_public_research_identities_for_program,
    load_public_research_release_projection, load_restricted_identity_linkage,
    persist_restricted_identity_linkage, RestrictedIdentityLinkagePersistenceDisposition,
    RestrictedIdentityLinkagePersistenceError,
};
use psychometrics_commons_runtime::research_identity_linkage::RestrictedIdentityLinkage;
use std::sync::{Mutex, MutexGuard};

static LINKAGE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn linkage_test_guard() -> MutexGuard<'static, ()> {
    LINKAGE_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS research_identity_linkage_test;\
             SET search_path TO research_identity_linkage_test;",
        )
        .unwrap();
    client
}

fn reset_linkage_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP VIEW IF EXISTS research_identity_linkage_test.public_research_identity;\
             DROP TABLE IF EXISTS research_identity_linkage_test.research_identity_linkage;\
             DROP TABLE IF EXISTS research_identity_linkage_test.research_participant;",
        )
        .unwrap();
}

fn sample_linkage() -> RestrictedIdentityLinkage {
    RestrictedIdentityLinkage::new(
        "linkage_commons_program_one",
        "participant_operational_one",
        "research_participant_program_one",
        "research_program_commons_one",
        "linkage_key_version_2026_q3",
        1_724_000_000_000,
    )
    .unwrap()
}

#[test]
fn persist_and_load_keeps_authorized_linkage_and_omits_it_from_public_projection() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    apply_research_identity_linkage_migration(&mut client).unwrap();

    let linkage = sample_linkage();
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_restricted_identity_linkage(&mut transaction, &linkage).unwrap();
    assert_eq!(
        disposition,
        RestrictedIdentityLinkagePersistenceDisposition::Inserted
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let loaded = load_restricted_identity_linkage(&mut transaction, linkage.linkage_ref())
        .unwrap()
        .expect("authorized load must return the stored linkage");
    assert_eq!(loaded, linkage);

    let projection =
        load_public_research_release_projection(&mut transaction, linkage.linkage_ref())
            .unwrap()
            .expect("public projection must exist after persist");
    assert_eq!(
        projection.research_participant_ref(),
        linkage.research_participant_ref()
    );
    assert_eq!(
        projection.research_program_ref(),
        linkage.research_program_ref()
    );
    let rendered = format!("{projection:?}");
    assert!(!rendered.contains(linkage.participant_ref()));
    assert!(!rendered.contains(linkage.linkage_key_version()));
    transaction.commit().unwrap();

    let public_columns: Vec<String> = client
        .query(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = 'research_identity_linkage_test' \
               AND table_name = 'public_research_identity' \
             ORDER BY column_name",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        public_columns,
        vec![
            "research_participant_ref".to_owned(),
            "research_program_ref".to_owned()
        ]
    );
}

#[test]
fn exact_replay_is_idempotent_and_conflicting_rebinding_fails_closed() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    apply_research_identity_linkage_migration(&mut client).unwrap();
    let linkage = sample_linkage();

    let mut transaction = client.transaction().unwrap();
    persist_restricted_identity_linkage(&mut transaction, &linkage).unwrap();
    let replay = persist_restricted_identity_linkage(&mut transaction, &linkage).unwrap();
    assert_eq!(
        replay,
        RestrictedIdentityLinkagePersistenceDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let conflicting = RestrictedIdentityLinkage::new(
        linkage.linkage_ref(),
        "participant_operational_two",
        linkage.research_participant_ref(),
        linkage.research_program_ref(),
        linkage.linkage_key_version(),
        linkage.recorded_at_unix_ms(),
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_restricted_identity_linkage(&mut transaction, &conflicting)
        .expect_err("rebinding a linkage identity must fail closed");
    assert!(matches!(
        error,
        RestrictedIdentityLinkagePersistenceError::ConflictingReplay
    ));
    transaction.rollback().unwrap();

    let same_program = RestrictedIdentityLinkage::new(
        "linkage_commons_program_conflict",
        linkage.participant_ref(),
        "research_participant_program_conflict",
        linkage.research_program_ref(),
        linkage.linkage_key_version(),
        1_724_000_100_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_restricted_identity_linkage(&mut transaction, &same_program)
        .expect_err("one operational participant may not receive two identities in one program");
    assert!(matches!(
        error,
        RestrictedIdentityLinkagePersistenceError::ConflictingReplay
    ));
}

#[test]
fn second_program_persists_a_distinct_research_identity_for_the_same_person() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    apply_research_identity_linkage_migration(&mut client).unwrap();
    let first = sample_linkage();
    let second = RestrictedIdentityLinkage::new(
        "linkage_commons_program_two",
        first.participant_ref(),
        "research_participant_program_two",
        "research_program_commons_two",
        "linkage_key_version_2026_q3",
        1_724_000_100_000,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    persist_restricted_identity_linkage(&mut transaction, &first).unwrap();
    let disposition = persist_restricted_identity_linkage(&mut transaction, &second).unwrap();
    assert_eq!(
        disposition,
        RestrictedIdentityLinkagePersistenceDisposition::Inserted
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let loaded = load_restricted_identity_linkage(&mut transaction, second.linkage_ref())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.participant_ref(), first.participant_ref());
    assert_ne!(
        loaded.research_participant_ref(),
        first.research_participant_ref()
    );
}

#[test]
fn serializable_isolation_is_rejected_and_missing_rows_stay_absent() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    apply_research_identity_linkage_migration(&mut client).unwrap();
    let linkage = sample_linkage();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    let error = persist_restricted_identity_linkage(&mut transaction, &linkage)
        .expect_err("restricted linkage persist requires read committed");
    assert!(matches!(
        error,
        RestrictedIdentityLinkagePersistenceError::UnsupportedIsolationLevel
    ));
    transaction.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_restricted_identity_linkage(&mut transaction, " "),
        Err(RestrictedIdentityLinkagePersistenceError::InvalidReference)
    ));
    assert!(
        load_restricted_identity_linkage(&mut transaction, "linkage_missing_one")
            .unwrap()
            .is_none()
    );
    assert!(
        load_public_research_release_projection(&mut transaction, "linkage_missing_one")
            .unwrap()
            .is_none()
    );
}

#[test]
fn rebinding_a_research_participant_to_another_program_fails_closed() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    apply_research_identity_linkage_migration(&mut client).unwrap();
    client
        .execute(
            "INSERT INTO research_participant (\
                 research_participant_ref, research_program_ref, recorded_at_unix_ms\
             ) VALUES ($1, $2, $3)",
            &[
                &"research_participant_program_one",
                &"research_program_other_one",
                &1_724_000_000_000_i64,
            ],
        )
        .unwrap();

    let linkage = sample_linkage();
    let mut transaction = client.transaction().unwrap();
    let error = persist_restricted_identity_linkage(&mut transaction, &linkage)
        .expect_err("a research participant cannot change program on replay");
    assert!(matches!(
        error,
        RestrictedIdentityLinkagePersistenceError::ConflictingReplay
    ));
}

#[test]
fn persistence_database_errors_keep_a_source_and_stable_message() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    let linkage = sample_linkage();
    let mut transaction = client.transaction().unwrap();
    let error = persist_restricted_identity_linkage(&mut transaction, &linkage)
        .expect_err("missing relations must surface as a database failure");
    assert!(matches!(
        error,
        RestrictedIdentityLinkagePersistenceError::Database(_)
    ));
    assert_eq!(
        error.to_string(),
        "PostgreSQL restricted-linkage persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn program_scoped_public_load_reads_only_the_public_view() {
    let _guard = linkage_test_guard();
    let mut client = test_client();
    reset_linkage_tables(&mut client);
    apply_research_identity_linkage_migration(&mut client).unwrap();
    let first = sample_linkage();
    let second = RestrictedIdentityLinkage::new(
        "linkage_commons_program_two",
        first.participant_ref(),
        "research_participant_program_two",
        "research_program_commons_two",
        "linkage_key_version_2026_q3",
        1_724_000_100_000,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    persist_restricted_identity_linkage(&mut transaction, &first).unwrap();
    persist_restricted_identity_linkage(&mut transaction, &second).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let program_one =
        load_public_research_identities_for_program(&mut transaction, first.research_program_ref())
            .unwrap();
    assert_eq!(program_one.len(), 1);
    assert_eq!(
        program_one[0].research_participant_ref(),
        first.research_participant_ref()
    );
    assert_eq!(
        program_one[0].research_program_ref(),
        first.research_program_ref()
    );
    let rendered = format!("{program_one:?}");
    assert!(!rendered.contains(first.participant_ref()));
    assert!(!rendered.contains(first.linkage_key_version()));
    assert!(!rendered.contains(second.research_participant_ref()));

    let view_rows: Vec<(String, String)> = transaction
        .query(
            "SELECT research_participant_ref, research_program_ref \
             FROM research_identity_linkage_test.public_research_identity \
             WHERE research_program_ref = $1",
            &[&first.research_program_ref()],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(
        view_rows,
        vec![(
            first.research_participant_ref().to_owned(),
            first.research_program_ref().to_owned()
        )]
    );
    assert!(matches!(
        load_public_research_identities_for_program(
            &mut transaction,
            " research_program_commons_one"
        ),
        Err(RestrictedIdentityLinkagePersistenceError::InvalidReference)
    ));
    transaction.commit().unwrap();
}
