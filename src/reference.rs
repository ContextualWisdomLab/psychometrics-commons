//! Internal normalization for opaque product references.

/// Return a trimmed opaque reference or `None` when the input is blank, numeric-like, or unsafe.
///
/// Public references must contain meaningful nonnumeric identity material. The guard
/// therefore rejects ordinary numbers as well as signed, decimal, scientific-notation,
/// and Unicode-numeric spellings instead of accepting them as opaque identifiers. Embedded
/// control characters are also rejected so accepted references remain safe to serialize
/// into machine-readable and participant-facing product artifacts.
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
    let contains_control = normalized.chars().any(char::is_control);
    if normalized.is_empty() || numeric_like || contains_control {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::normalized_reference;

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
}
