//! Static guardrails for the integration migration's single-batch atomicity contract.

const MIGRATIONS: [(&str, &str); 2] = [
    (
        "0001_integration_delivery.sql",
        include_str!("../migrations/0001_integration_delivery.sql"),
    ),
    (
        "0013_outbox_delivery_lease.sql",
        include_str!("../migrations/0013_outbox_delivery_lease.sql"),
    ),
];

fn executable_line(line: &str) -> &str {
    line.split_once("--")
        .map_or(line, |(statement, _comment)| statement)
        .trim()
}

fn transaction_control_is_rejected(sql: &str) -> bool {
    let statement = executable_line(sql).to_ascii_uppercase();
    statement.starts_with("BEGIN;")
        || statement.starts_with("START TRANSACTION")
        || statement.starts_with("COMMIT;")
        || statement.starts_with("ROLLBACK;")
}

#[test]
fn transaction_control_variants_are_rejected_without_flagging_do_blocks() {
    for sql in [
        "BEGIN TRANSACTION;",
        "BEGIN WORK;",
        "COMMIT WORK;",
        "ROLLBACK WORK;",
        "CREATE TABLE example_table (id INTEGER); BEGIN;",
    ] {
        assert!(
            transaction_control_is_rejected(sql),
            "top-level transaction control must be rejected even when it is a PostgreSQL syntax variant or follows another statement: {sql}"
        );
    }

    assert!(
        !transaction_control_is_rejected("DO $$ BEGIN PERFORM 1; END $$;"),
        "PL/pgSQL BEGIN/END inside a DO dollar-quoted body is not top-level transaction control"
    );
}

#[test]
fn integration_migration_fragments_cannot_break_single_batch_atomicity() {
    for (name, sql) in MIGRATIONS {
        assert!(
            !sql.to_ascii_uppercase().contains("CONCURRENTLY"),
            "{name} must not add CONCURRENTLY operations because they cannot run inside the implicit transaction used by apply_integration_migration"
        );

        for line in sql
            .lines()
            .map(executable_line)
            .filter(|line| !line.is_empty())
        {
            assert!(
                !transaction_control_is_rejected(line),
                "{name} must not contain top-level transaction control because apply_integration_migration relies on one simple-query batch: {line}"
            );
        }
    }
}
