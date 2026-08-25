//! Authorization composition for participant-owned personal result exports.
//!
//! A personal export intentionally contains the participant reference and the same
//! continuous scores as its immutable result snapshot. That is useful to the owner,
//! but it also means an adapter must not return an export merely because the caller
//! supplied a matching result identifier. This module first authorizes the stored
//! result using its product-owned tenant and participant records, then verifies that
//! the export is bound to that exact result and owner.
//!
//! The ordering is deliberate: an unauthorized caller receives the ordinary result
//! authorization failure before export-binding details are evaluated. This avoids
//! turning a mismatched export into an existence oracle across tenants or owners.

use crate::authorization::{AuthorizationContext, AuthorizationError};
use crate::participant::ParticipantRecord;
use crate::result::ResultSnapshot;
use crate::result_authorization::authorize_result_read;
use crate::result_export::ResultExport;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error returned when authorizing one personal result export read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResultExportAuthorizationError {
    /// The stored result was not authorized for the authenticated product context.
    Authorization(AuthorizationError),
    /// The export does not identify the exact stored result and participant owner.
    ExportBindingMismatch,
}

impl Display for ResultExportAuthorizationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authorization(error) => Display::fmt(error, formatter),
            Self::ExportBindingMismatch => formatter.write_str(
                "personal result export does not belong to the authorized immutable result",
            ),
        }
    }
}

impl Error for ResultExportAuthorizationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(error) => Some(error),
            Self::ExportBindingMismatch => None,
        }
    }
}

impl From<AuthorizationError> for ResultExportAuthorizationError {
    fn from(error: AuthorizationError) -> Self {
        Self::Authorization(error)
    }
}

/// Authorize delivery of one personal export for an immutable stored result.
///
/// The participant record supplies the server-owned tenant identity, while the
/// result snapshot supplies the owner and result identity. The generic result-read
/// authorization therefore runs before this function checks the export binding.
/// Once access to the stored result is proven, both the export's result reference
/// and copied participant reference must exactly match that result.
///
/// This function does not create an export, load persistence, or perform HTTP work.
/// It is the domain guard an adapter can call after loading the stored records and
/// before returning the already-created personal export.
///
/// # Errors
///
/// Returns [`ResultExportAuthorizationError::Authorization`] when normal result
/// authorization fails. Returns
/// [`ResultExportAuthorizationError::ExportBindingMismatch`] when the authenticated
/// caller may read the supplied result but the export was created from another
/// result or participant owner.
pub fn authorize_result_export_read(
    actor: &AuthorizationContext,
    participant: &ParticipantRecord,
    result: &ResultSnapshot,
    export: &ResultExport,
) -> Result<(), ResultExportAuthorizationError> {
    authorize_result_read(actor, participant, result)?;

    if export.result_snapshot_ref() != result.result_snapshot_ref()
        || export.participant_ref() != result.participant_ref()
    {
        return Err(ResultExportAuthorizationError::ExportBindingMismatch);
    }

    Ok(())
}
