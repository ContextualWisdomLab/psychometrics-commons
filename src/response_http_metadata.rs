//! Shared HTTP response metadata for the public response-write surface.
//!
//! The in-process adapter and socket serializer must expose the same method
//! rejection metadata. Keeping this on the public response value lets embedding
//! hosts faithfully render RFC 9110 `Allow` semantics without depending on the
//! bundled socket writer.

use crate::response_http::ResponseHttpResponse;

impl ResponseHttpResponse {
    /// Return the RFC 9110 `Allow` field value when the response rejects a method.
    ///
    /// The response-write collection currently implements only `POST`, so its
    /// method-rejection response advertises exactly that method. Ordinary success
    /// and problem responses do not carry `Allow` metadata.
    #[must_use]
    pub const fn allow(&self) -> Option<&'static str> {
        if self.status() == 405 {
            Some("POST")
        } else {
            None
        }
    }
}
