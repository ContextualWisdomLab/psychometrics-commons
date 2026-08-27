//! Internal normalization for opaque product references.

/// Return a trimmed opaque reference or `None` when the input is blank, numeric-like, or unsafe.
///
/// Public references must contain meaningful nonnumeric identity material. The guard
/// rejects ordinary numbers as well as signed, decimal, scientific-notation, and
/// Unicode-numeric spellings instead of accepting them as opaque identifiers. Embedded
/// control characters are rejected. Unicode `Default_Ignorable_Code_Point` characters
/// are also rejected because they can be invisible or change display behavior while
/// leaving a byte-distinct identifier, which is unsafe for authorization, replay, audit,
/// and participant-facing artifacts. Ordinary visible multilingual characters remain valid.
#[must_use]
pub(crate) fn normalized_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    let numeric_like = normalized.chars().any(char::is_numeric)
        && normalized.chars().all(|character| {
            character.is_numeric()
                || matches!(
                    character,
                    '+' | '-'
                        | '.'
                        | ','
                        | 'e'
                        | 'E'
                        | '\u{066B}'
                        | '\u{066C}'
                        | '\u{FF0E}'
                        | '\u{FF0C}'
                )
        });
    let contains_unsafe_character = normalized.chars().any(|character| {
        character.is_control() || is_default_ignorable_identifier_character(character)
    });
    if normalized.is_empty() || numeric_like || contains_unsafe_character {
        None
    } else {
        Some(normalized)
    }
}

/// Return whether a character is Unicode 17.0 default-ignorable identifier evidence.
///
/// These code points are normally invisible or formatting-only. Keeping the Unicode 17.0
/// ranges explicit makes an upgrade deliberate instead of silently changing which external
/// identifiers the product accepts when the Rust toolchain changes Unicode tables.
const fn is_default_ignorable_identifier_character(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::normalized_reference;
    use crate::postgres_integration::apply_integration_migration;
    use postgres::{Client, NoTls};

    const SCALAR_PARITY_BATCH_SIZE: usize = 32_768;

    fn reference_parity_client() -> Client {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS integration_reference_domain_parity_test CASCADE; \
                 CREATE SCHEMA integration_reference_domain_parity_test; \
                 SET search_path TO integration_reference_domain_parity_test;",
            )
            .unwrap();
        apply_integration_migration(&mut client).unwrap();
        client
    }

    fn assert_postgres_parity_batch(
        client: &mut Client,
        references: &[String],
        expected: &[bool],
    ) {
        let mismatches: Vec<String> = client
            .query_one(
                "SELECT COALESCE(array_agg(reference_text), ARRAY[]::text[]) \
                 FROM (\
                     SELECT reference_text \
                     FROM unnest($1::text[], $2::boolean[]) \
                          AS candidate(reference_text, expected_valid) \
                     WHERE integration_reference_is_valid(reference_text) \
                           IS DISTINCT FROM expected_valid \
                     LIMIT 8\
                 ) AS mismatch",
                &[&references, &expected],
            )
            .expect("the persisted reference validator must be callable")
            .get(0);
        assert!(
            mismatches.is_empty(),
            "PostgreSQL reference classification diverged from normalized_reference for {mismatches:?}"
        );
    }

    #[test]
    fn opaque_references_reject_embedded_control_characters() {
        assert_eq!(normalized_reference("participant_\u{0001}_account"), None);
        assert_eq!(
            normalized_reference("construct_\u{001f}_extraversion"),
            None
        );
        assert_eq!(
            normalized_reference("  construct_extraversion  "),
            Some("construct_extraversion")
        );
    }

    #[test]
    fn opaque_references_reject_default_ignorable_aliases() {
        for reference in [
            "participant\u{200b}_account",
            "participant\u{200d}_account",
            "participant\u{202e}_account",
            "participant\u{2060}_account",
            "participant\u{fe0f}_account",
            "participant\u{e0001}_account",
        ] {
            assert_eq!(normalized_reference(reference), None, "{reference:?}");
        }
    }

    #[test]
    fn opaque_references_preserve_visible_multilingual_material() {
        assert_eq!(
            normalized_reference("participant_ref_가나다_東京_éclair"),
            Some("participant_ref_가나다_東京_éclair")
        );
    }

    #[test]
    fn postgres_validator_matches_domain_reference_normalization_for_every_unicode_scalar() {
        let mut client = reference_parity_client();
        let mut references = Vec::with_capacity(SCALAR_PARITY_BATCH_SIZE);
        let mut expected = Vec::with_capacity(SCALAR_PARITY_BATCH_SIZE);
        let mut checked_scalars = 0usize;

        for character in (0..=char::MAX as u32).filter_map(char::from_u32) {
            if character == '\0' {
                // PostgreSQL text cannot represent U+0000. Other representable controls are
                // exercised through the same domain validator in this exhaustive comparison.
                continue;
            }
            let reference = character.to_string();
            expected.push(normalized_reference(&reference) == Some(reference.as_str()));
            references.push(reference);
            checked_scalars += 1;

            if references.len() == SCALAR_PARITY_BATCH_SIZE {
                assert_postgres_parity_batch(&mut client, &references, &expected);
                references.clear();
                expected.clear();
            }
        }

        if !references.is_empty() {
            assert_postgres_parity_batch(&mut client, &references, &expected);
        }
        assert!(checked_scalars > 1_000_000);
    }
}
