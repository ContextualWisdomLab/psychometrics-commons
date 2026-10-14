//! Guard the consent SQL parity test against positional multirange extraction.
//!
//! The migration contains more than one `ascii(character_text) <@ ...` expression.
//! This contract resolves the range by its SQL alias so reordering or adding another
//! classification cannot silently make a parity test inspect the wrong literal.

const MIGRATION: &str = include_str!("../migrations/0005_consent_lifecycle.sql");
const REFERENCE_SOURCE: &str = include_str!("../src/reference.rs");

const RANGE_PREFIX: &str = "ascii(character_text) <@ '";
const RANGE_SUFFIX: &str = "'::int4multirange";

fn parse_multirange(literal: &str) -> Vec<(u32, u32)> {
    let body = literal
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .expect("consent migration multirange must use canonical braces");

    body.split("),")
        .map(|range| {
            let range = range
                .strip_prefix('[')
                .expect("multirange entries must be inclusive-exclusive ranges");
            let range = range.strip_suffix(')').unwrap_or(range);
            let (start, end) = range
                .split_once(',')
                .expect("multirange entries must have start and end bounds");
            (
                start.parse().expect("multirange start must be u32"),
                end.parse().expect("multirange end must be u32"),
            )
        })
        .collect()
}

fn migration_ranges_bound_to(alias: &str) -> Vec<(u32, u32)> {
    let alias_marker = format!("\n                AS {alias}");
    let before_alias = MIGRATION
        .split_once(&alias_marker)
        .unwrap_or_else(|| panic!("consent migration must bind a multirange to {alias}"))
        .0;
    let literal_with_suffix = before_alias
        .rsplit_once(RANGE_PREFIX)
        .unwrap_or_else(|| panic!("{alias} must be bound to an ascii(character_text) multirange"))
        .1;
    let literal = literal_with_suffix
        .split_once(RANGE_SUFFIX)
        .unwrap_or_else(|| panic!("{alias} must use int4multirange"))
        .0;
    parse_multirange(literal)
}

fn codepoint_from_rust_unicode_literal(literal: &str) -> u32 {
    let prefix = literal
        .find("\\u{")
        .expect("default-ignorable Rust entry must use a Unicode escape")
        + 3;
    let suffix = literal[prefix..]
        .find('}')
        .expect("default-ignorable Rust Unicode escape must close")
        + prefix;
    u32::from_str_radix(&literal[prefix..suffix], 16)
        .expect("default-ignorable Rust Unicode escape must be hexadecimal")
}

fn rust_default_ignorable_ranges() -> Vec<(u32, u32)> {
    let function = REFERENCE_SOURCE
        .split_once("const fn is_default_ignorable_identifier_character")
        .expect("reference boundary must declare the pinned default-ignorable helper")
        .1;
    let patterns = function
        .split_once("#[cfg(test)]")
        .expect("reference boundary helper must precede its unit tests")
        .0;

    patterns
        .lines()
        .filter(|line| line.contains("\\u{"))
        .map(|line| {
            let pattern = line.trim().trim_start_matches('|').trim();
            if let Some((start, end)) = pattern.split_once("..=") {
                (
                    codepoint_from_rust_unicode_literal(start),
                    codepoint_from_rust_unicode_literal(end) + 1,
                )
            } else {
                let codepoint = codepoint_from_rust_unicode_literal(pattern);
                (codepoint, codepoint + 1)
            }
        })
        .collect()
}

#[test]
fn default_ignorable_parity_is_bound_to_the_named_sql_classification() {
    assert_eq!(
        migration_ranges_bound_to("is_default_ignorable"),
        rust_default_ignorable_ranges()
    );
}
