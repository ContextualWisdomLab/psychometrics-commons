//! Internal validation for opaque product references.

/// Return an opaque reference only when the supplied spelling is already canonical.
///
/// Public references must contain meaningful nonnumeric identity material. The guard
/// rejects leading or trailing Unicode whitespace rather than silently normalizing a
/// byte-distinct external identity. It also rejects ordinary numbers as well as signed,
/// decimal, scientific-notation, and Unicode-numeric spellings instead of accepting them
/// as opaque identifiers.
#[must_use]
pub(crate) fn normalized_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    if normalized != reference {
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
