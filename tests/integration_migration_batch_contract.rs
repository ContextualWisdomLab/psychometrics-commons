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
            let statement = line.to_ascii_uppercase();
            assert!(
                !statement.starts_with("BEGIN;")
                    && !statement.starts_with("START TRANSACTION")
                    && !statement.starts_with("COMMIT;")
                    && !statement.starts_with("ROLLBACK;"),
                "{name} must not contain top-level transaction control because apply_integration_migration relies on one simple-query batch: {line}"
            );
        }
    }
}
