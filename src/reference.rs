//! Internal normalization for opaque product references.

/// Return a trimmed opaque reference or `None` when the input is blank or numeric-only.
#[must_use]
pub(crate) fn normalized_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    if normalized.is_empty()
        || normalized
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        None
    } else {
        Some(normalized)
    }
}
