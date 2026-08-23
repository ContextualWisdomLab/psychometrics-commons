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

fn dollar_quote_delimiter(sql: &[u8], start: usize) -> Option<Vec<u8>> {
    if sql.get(start) != Some(&b'$') {
        return None;
    }

    let mut end = start + 1;
    while let Some(byte) = sql.get(end) {
        if *byte == b'$' {
            if end == start + 1 || sql[start + 1].is_ascii_alphabetic() || sql[start + 1] == b'_' {
                return Some(sql[start..=end].to_vec());
            }
            return None;
        }
        if !byte.is_ascii_alphanumeric() && *byte != b'_' {
            return None;
        }
        end += 1;
    }
    None
}

fn top_level_statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut statement = String::new();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                statement.push(' ');
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut depth = 1usize;
                while index < bytes.len() && depth > 0 {
                    if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                        depth += 1;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                statement.push(' ');
            }
            quote @ (b'\'' | b'"') => {
                statement.push(' ');
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == quote {
                        if bytes.get(index + 1) == Some(&quote) {
                            index += 2;
                        } else {
                            index += 1;
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
                statement.push(' ');
            }
            b'$' => {
                if let Some(delimiter) = dollar_quote_delimiter(bytes, index) {
                    statement.push(' ');
                    index += delimiter.len();
                    while index + delimiter.len() <= bytes.len() {
                        if bytes[index..].starts_with(&delimiter) {
                            index += delimiter.len();
                            break;
                        }
                        index += 1;
                    }
                    statement.push(' ');
                } else {
                    statement.push('$');
                    index += 1;
                }
            }
            b';' => {
                if !statement.trim().is_empty() {
                    statements.push(statement.trim().to_owned());
                }
                statement.clear();
                index += 1;
            }
            byte => {
                statement.push(byte as char);
                index += 1;
            }
        }
    }

    if !statement.trim().is_empty() {
        statements.push(statement.trim().to_owned());
    }
    statements
}

fn statement_tokens(statement: &str) -> Vec<String> {
    statement
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_uppercase)
        .collect()
}

fn is_transaction_control(statement: &str) -> bool {
    let tokens = statement_tokens(statement);
    match tokens.as_slice() {
        [first, ..]
            if matches!(
                first.as_str(),
                "BEGIN" | "COMMIT" | "END" | "ROLLBACK" | "ABORT"
            ) =>
        {
            true
        }
        [first, second, ..]
            if (first == "START" && second == "TRANSACTION")
                || (first == "PREPARE" && second == "TRANSACTION") =>
        {
            true
        }
        _ => false,
    }
}

fn contains_concurrently(statement: &str) -> bool {
    statement_tokens(statement)
        .iter()
        .any(|token| token == "CONCURRENTLY")
}

#[test]
fn transaction_control_variants_are_rejected_without_flagging_do_blocks() {
    for sql in [
        "BEGIN;",
        "BEGIN TRANSACTION;",
        "BEGIN WORK;",
        "START TRANSACTION;",
        "COMMIT;",
        "COMMIT WORK;",
        "END WORK;",
        "ROLLBACK;",
        "ROLLBACK WORK;",
        "ABORT WORK;",
        "PREPARE TRANSACTION 'migration';",
        "CREATE TABLE example_table (id INTEGER); BEGIN;",
    ] {
        assert!(
            top_level_statements(sql).iter().any(|statement| is_transaction_control(statement)),
            "top-level transaction control must be detected even when it is a PostgreSQL syntax variant or follows another statement: {sql}"
        );
    }

    let do_block = "DO $$ BEGIN PERFORM 1; END $$;";
    assert!(
        top_level_statements(do_block)
            .iter()
            .all(|statement| !is_transaction_control(statement)),
        "PL/pgSQL BEGIN/END inside a DO dollar-quoted body is not top-level transaction control"
    );
}

#[test]
fn comments_and_literals_cannot_manufacture_transaction_control() {
    let sql = "-- BEGIN;\nSELECT 'COMMIT WORK;', \"ROLLBACK\"; /* START TRANSACTION; */";
    assert!(
        top_level_statements(sql)
            .iter()
            .all(|statement| !is_transaction_control(statement)),
        "transaction-control words in comments and quoted values are not executable top-level statements"
    );
}

#[test]
fn integration_migration_fragments_cannot_break_single_batch_atomicity() {
    for (name, sql) in MIGRATIONS {
        for statement in top_level_statements(sql) {
            assert!(
                !is_transaction_control(&statement),
                "{name} must not contain top-level transaction control because apply_integration_migration relies on one simple-query batch: {statement}"
            );
            assert!(
                !contains_concurrently(&statement),
                "{name} must not add top-level CONCURRENTLY operations because they cannot run inside the implicit transaction used by apply_integration_migration: {statement}"
            );
        }
    }
}
