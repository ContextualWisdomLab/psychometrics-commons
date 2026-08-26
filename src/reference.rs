//! Internal validation for opaque product references.

/// Return an opaque reference only when the supplied spelling is already canonical.
///
/// Public references must contain meaningful nonnumeric identity material. The guard
/// rejects leading or trailing Unicode whitespace rather than silently normalizing a
/// byte-distinct external identity, and rejects control characters plus Unicode 17.0
/// `Default_Ignorable_Code_Point` characters anywhere so public identifiers cannot carry
/// line breaks, NULs, escape sequences, hidden joiners, variation selectors, tag characters,
/// or bidirectional display controls into audit and transport surfaces. Unicode UTS #39 treats
/// default-ignorable identifier characters as restricted for security profiles. The guard also
/// rejects ordinary numbers as well as signed, decimal, scientific-notation, and Unicode-numeric
/// spellings instead of accepting them as opaque identifiers.
#[must_use]
pub(crate) fn canonical_opaque_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    if normalized != reference
        || reference.chars().any(|character| {
            character.is_control() || is_default_ignorable_identifier_character(character)
        })
    {
        return None;
    }

    let numeric_like = reference.chars().any(char::is_numeric)
        && reference.chars().all(|character| {
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
    if reference.is_empty() || numeric_like {
        None
    } else {
        Some(reference)
    }
}

/// Return whether a character is Unicode 17.0 `Default_Ignorable_Code_Point` evidence.
///
/// The ranges mirror the normative Unicode Character Database derived property used by UTS #39
/// security profiles. Keeping the list explicit avoids silently accepting newly invisible aliases
/// when the Rust toolchain changes its Unicode tables; a Unicode-version update therefore requires
/// an intentional source and regression-test change. This does not restrict ordinary visible
/// scripts used by upstream issuers.
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
    use super::canonical_opaque_reference;

    #[test]
    fn opaque_references_reject_embedded_control_characters() {
        assert_eq!(
            canonical_opaque_reference("participant_\u{0001}_account"),
            None
        );
        assert_eq!(
            canonical_opaque_reference("construct_\u{001f}_extraversion"),
            None
        );
        assert_eq!(
            canonical_opaque_reference("  construct_extraversion  "),
            None
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
            assert_eq!(canonical_opaque_reference(reference), None, "{reference:?}");
        }
    }

    #[test]
    fn opaque_references_preserve_visible_multilingual_material() {
        assert_eq!(
            canonical_opaque_reference("participant_ref_가나다_東京_éclair"),
            Some("participant_ref_가나다_東京_éclair")
        );
    }
}
