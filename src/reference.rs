//! Internal normalization for opaque product references.

/// Return a trimmed opaque reference or `None` when the input is blank or numeric-like.
///
/// Public references must contain meaningful nonnumeric identity material. The guard
/// therefore rejects ordinary numbers as well as signed, decimal, scientific-notation,
/// and Unicode-numeric spellings instead of accepting them as opaque identifiers.
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
    if normalized.is_empty() || numeric_like {
        None
    } else {
        Some(normalized)
    }
}
