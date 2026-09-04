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
    use super::{is_default_ignorable_identifier_character, normalized_reference};

    fn assessment_session_default_ignorable_ranges() -> Vec<(u32, u32)> {
        let migration = include_str!("../migrations/0014_assessment_session.sql");
        let alias_index = migration
            .find("AS is_default_ignorable")
            .expect("assessment-session migration must name the default-ignorable classifier");
        let classifier_prefix = &migration[..alias_index];
        let literal_start = classifier_prefix
            .rfind("'{")
            .expect("default-ignorable classifier must use an int4multirange literal")
            + 2;
        let literal_end = classifier_prefix
            .rfind("}'::int4multirange")
            .expect("default-ignorable classifier must terminate its int4multirange literal");

        migration[literal_start..literal_end]
            .split("),")
            .map(|raw_range| {
                let raw_range = raw_range
                    .trim()
                    .strip_prefix('[')
                    .expect("default-ignorable ranges must have inclusive lower bounds")
                    .trim_end_matches(')');
                let (start, end) = raw_range
                    .split_once(',')
                    .expect("default-ignorable ranges must contain lower and upper bounds");
                (
                    start
                        .parse::<u32>()
                        .expect("default-ignorable lower bound must be an integer"),
                    end.parse::<u32>()
                        .expect("default-ignorable upper bound must be an integer"),
                )
            })
            .collect()
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
    fn assessment_session_sql_default_ignorable_ranges_match_the_real_rust_guard() {
        let sql_ranges = assessment_session_default_ignorable_ranges();

        for character in (0u32..=0x0010_FFFF).filter_map(char::from_u32) {
            let code_point = u32::from(character);
            let sql_classifies_default_ignorable = sql_ranges
                .iter()
                .any(|(start, end)| *start <= code_point && code_point < *end);
            assert_eq!(
                sql_classifies_default_ignorable,
                is_default_ignorable_identifier_character(character),
                "assessment-session SQL/Rust default-ignorable drift at U+{code_point:04X}"
            );
        }
    }

    #[test]
    fn opaque_references_preserve_visible_multilingual_material() {
        assert_eq!(
            normalized_reference("participant_ref_가나다_東京_éclair"),
            Some("participant_ref_가나다_東京_éclair")
        );
    }
}
