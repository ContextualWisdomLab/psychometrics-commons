//! Internal normalization for opaque product references.

/// Return a non-empty trimmed reference or `None` when the input is blank.
#[must_use]
pub(crate) fn normalized_reference(reference: &str) -> Option<&str> {
    let normalized = reference.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}
