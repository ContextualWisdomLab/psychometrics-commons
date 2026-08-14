//! Internal validation for opaque product references.

/// Return an opaque reference only when it already uses canonical spelling.
///
/// Public references must contain meaningful nonnumeric identity material and must not
/// rely on transport-side whitespace normalization. Rejecting padded aliases prevents two
/// byte-distinct external identifiers from collapsing onto the same authorization,
/// idempotency, persistence, or audit identity. The guard also rejects ordinary numbers as
/// well as signed, decimal, scientific-notation, and Unicode-numeric spellings.
#[must_use]
pub(crate) fn normalized_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    if normalized != reference {
        return None;
    }

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
        Some(reference)
    }
}
