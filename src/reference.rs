//! Internal validation for opaque product references.

/// Return an opaque reference only when the supplied spelling is already canonical.
///
/// Public references must contain meaningful nonnumeric identity material. The guard
/// rejects leading or trailing Unicode whitespace rather than silently normalizing a
/// byte-distinct external identity, and rejects control characters plus security-sensitive
/// invisible/directional format controls anywhere so public identifiers cannot carry line
/// breaks, NULs, escape sequences, hidden joiners, or bidirectional display overrides into
/// audit and transport surfaces. Unicode UTS #39 treats these default-ignorable identifier
/// characters as restricted for security profiles. The guard also rejects ordinary numbers
/// as well as signed, decimal, scientific-notation, and Unicode-numeric spellings instead of
/// accepting them as opaque identifiers.
#[must_use]
pub(crate) fn normalized_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    if normalized != reference
        || reference
            .chars()
            .any(|character| character.is_control() || is_unsafe_identifier_format(character))
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

/// Return whether a Unicode format character can invisibly alter or reorder an identifier.
///
/// This intentionally covers the security-relevant zero-width, direction-mark, bidi-embedding,
/// bidi-isolate, and BOM controls used by spoofing/log-reordering attacks. It is narrower than a
/// full Unicode identifier profile so consuming domains do not accidentally redefine which
/// visible scripts an upstream issuer may use.
const fn is_unsafe_identifier_format(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
    )
}
