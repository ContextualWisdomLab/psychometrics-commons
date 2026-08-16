//! `PostgreSQL` 18 persistence for restricted research-identity linkage.
//!
//! Authorized research workflows persist and load the operational-to-research
//! mapping. Public release projections select only research identities. The
//! caller owns the connection, credentials, and transaction boundary. Replay
//! requires `READ COMMITTED` so a concurrent insert that wins a unique-key race
//! is visible to the exact-replay classifier.

use crate::reference::normalized_reference;
use crate::research_identity_linkage::{
    PublicResearchReleaseProjection, RestrictedIdentityLinkage,
};
use postgres::error::SqlState;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESEARCH_IDENTITY_LINKAGE_MIGRATION: &str =
    include_str!("../migrations/0025_research_identity_linkage.sql");

/// Outcome of persisting one restricted identity linkage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RestrictedIdentityLinkagePersistenceDisposition {
    /// A new research participant and linkage were inserted.
    Inserted,
    /// The same immutable linkage evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable restricted-linkage persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum RestrictedIdentityLinkagePersistenceError {
    /// A linkage, participant, program, or key-version identity was blank or numeric-like.
    InvalidReference,
    /// Linkage identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Restricted-linkage persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for RestrictedIdentityLinkagePersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "restricted linkage persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "restricted linkage identity was replayed with conflicting evidence"
            }
            Self::InvalidTimestamp => {
                "restricted linkage timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "restricted linkage persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL restricted-linkage persistence failed",
        })
    }
}

impl Error for RestrictedIdentityLinkagePersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for RestrictedIdentityLinkagePersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent restricted-linkage migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_research_identity_linkage_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(RESEARCH_IDENTITY_LINKAGE_MIGRATION)
}

/// Persist one restricted operational-to-research identity mapping.
///
/// Exact replay of the same linkage evidence is idempotent. Rebinding a linkage
/// identity, or assigning a second research identity to the same operational
/// participant in the same program, fails closed.
///
/// # Errors
///
/// Returns [`RestrictedIdentityLinkagePersistenceError`] for unsupported
/// isolation, conflicting replay, an invalid timestamp, or a database failure.
pub fn persist_restricted_identity_linkage(
    transaction: &mut Transaction<'_>,
    linkage: &RestrictedIdentityLinkage,
) -> Result<
    RestrictedIdentityLinkagePersistenceDisposition,
    RestrictedIdentityLinkagePersistenceError,
> {
    require_read_committed(transaction)?;
    let recorded_at = postgres_timestamp(linkage.recorded_at_unix_ms())?;
    persist_research_participant(transaction, linkage, recorded_at)?;
    let inserted = match transaction.execute(
        "INSERT INTO research_identity_linkage (\
             linkage_ref, participant_ref, research_participant_ref, \
             research_program_ref, linkage_key_version, recorded_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (linkage_ref) DO NOTHING",
        &[
            &linkage.linkage_ref(),
            &linkage.participant_ref(),
            &linkage.research_participant_ref(),
            &linkage.research_program_ref(),
            &linkage.linkage_key_version(),
            &recorded_at,
        ],
    ) {
        Ok(count) => count,
        Err(error) if is_unique_violation(&error) => {
            return Err(RestrictedIdentityLinkagePersistenceError::ConflictingReplay);
        }
        Err(error) => return Err(RestrictedIdentityLinkagePersistenceError::Database(error)),
    };
    if inserted == 1 {
        Ok(RestrictedIdentityLinkagePersistenceDisposition::Inserted)
    } else {
        classify_existing_linkage(transaction, linkage, recorded_at)
    }
}

/// Load one restricted linkage for an authorized research workflow.
///
/// # Errors
///
/// Returns [`RestrictedIdentityLinkagePersistenceError`] when the linkage
/// identity is invalid, stored evidence cannot be reconstructed, or the
/// database operation fails.
pub fn load_restricted_identity_linkage(
    client: &mut impl postgres::GenericClient,
    linkage_ref: &str,
) -> Result<Option<RestrictedIdentityLinkage>, RestrictedIdentityLinkagePersistenceError> {
    let linkage_ref = required_reference(linkage_ref)?;
    let row = client.query_opt(
        "SELECT participant_ref, research_participant_ref, research_program_ref, \
                linkage_key_version, recorded_at_unix_ms \
         FROM research_identity_linkage WHERE linkage_ref = $1",
        &[&linkage_ref],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };
    let recorded_at: i64 = row.get(4);
    reconstruct_stored_linkage(
        linkage_ref,
        &row.get::<_, String>(0),
        &row.get::<_, String>(1),
        &row.get::<_, String>(2),
        &row.get::<_, String>(3),
        recorded_at,
    )
    .map(Some)
}

/// Load the public-release projection for one stored linkage.
///
/// The returned value contains only research identities. It cannot carry the
/// operational participant or the linkage-key version. A public-release role
/// that can select only `public_research_identity` should call
/// [`load_public_research_identities_for_program`] instead; this lookup still
/// needs the restricted linkage identity.
///
/// # Errors
///
/// Returns [`RestrictedIdentityLinkagePersistenceError`] when the linkage
/// identity is invalid or the database operation fails.
pub fn load_public_research_release_projection(
    client: &mut impl postgres::GenericClient,
    linkage_ref: &str,
) -> Result<Option<PublicResearchReleaseProjection>, RestrictedIdentityLinkagePersistenceError> {
    let linkage_ref = required_reference(linkage_ref)?;
    let Some(linkage) = load_restricted_identity_linkage(client, linkage_ref)? else {
        return Ok(None);
    };
    Ok(Some(linkage.public_release_projection()))
}

/// Load public-release identities for one research program from the public view.
///
/// The query selects only `public_research_identity` columns. It cannot return
/// an operational participant or linkage-key version.
///
/// # Errors
///
/// Returns [`RestrictedIdentityLinkagePersistenceError`] when the program
/// identity is invalid, a stored row cannot be reconstructed, or the database
/// operation fails.
pub fn load_public_research_identities_for_program(
    client: &mut impl postgres::GenericClient,
    research_program_ref: &str,
) -> Result<Vec<PublicResearchReleaseProjection>, RestrictedIdentityLinkagePersistenceError> {
    let research_program_ref = required_reference(research_program_ref)?;
    let rows = client.query(
        "SELECT research_participant_ref, research_program_ref \
         FROM public_research_identity \
         WHERE research_program_ref = $1 \
         ORDER BY research_participant_ref",
        &[&research_program_ref],
    )?;
    rows.into_iter()
        .map(|row| {
            PublicResearchReleaseProjection::new(&row.get::<_, String>(0), &row.get::<_, String>(1))
                .map_err(|_| RestrictedIdentityLinkagePersistenceError::ConflictingReplay)
        })
        .collect()
}

fn persist_research_participant(
    transaction: &mut Transaction<'_>,
    linkage: &RestrictedIdentityLinkage,
    recorded_at: i64,
) -> Result<(), RestrictedIdentityLinkagePersistenceError> {
    let inserted = transaction.execute(
        "INSERT INTO research_participant (\
             research_participant_ref, research_program_ref, recorded_at_unix_ms\
         ) VALUES ($1, $2, $3) \
         ON CONFLICT (research_participant_ref) DO NOTHING",
        &[
            &linkage.research_participant_ref(),
            &linkage.research_program_ref(),
            &recorded_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(());
    }
    let row = transaction.query_one(
        "SELECT research_program_ref, recorded_at_unix_ms \
         FROM research_participant WHERE research_participant_ref = $1",
        &[&linkage.research_participant_ref()],
    )?;
    let stored_program: String = row.get(0);
    let stored_recorded_at: i64 = row.get(1);
    if stored_program == linkage.research_program_ref() && stored_recorded_at == recorded_at {
        Ok(())
    } else {
        Err(RestrictedIdentityLinkagePersistenceError::ConflictingReplay)
    }
}

fn classify_existing_linkage(
    transaction: &mut Transaction<'_>,
    linkage: &RestrictedIdentityLinkage,
    recorded_at: i64,
) -> Result<
    RestrictedIdentityLinkagePersistenceDisposition,
    RestrictedIdentityLinkagePersistenceError,
> {
    let row = transaction.query_one(
        "SELECT participant_ref, research_participant_ref, research_program_ref, \
                linkage_key_version, recorded_at_unix_ms \
         FROM research_identity_linkage WHERE linkage_ref = $1",
        &[&linkage.linkage_ref()],
    )?;
    let matches_stored = row.get::<_, String>(0) == linkage.participant_ref()
        && row.get::<_, String>(1) == linkage.research_participant_ref()
        && row.get::<_, String>(2) == linkage.research_program_ref()
        && row.get::<_, String>(3) == linkage.linkage_key_version()
        && row.get::<_, i64>(4) == recorded_at;
    if matches_stored {
        Ok(RestrictedIdentityLinkagePersistenceDisposition::Duplicate)
    } else {
        Err(RestrictedIdentityLinkagePersistenceError::ConflictingReplay)
    }
}

fn reconstruct_stored_linkage(
    linkage_ref: &str,
    participant_ref: &str,
    research_participant_ref: &str,
    research_program_ref: &str,
    linkage_key_version: &str,
    recorded_at: i64,
) -> Result<RestrictedIdentityLinkage, RestrictedIdentityLinkagePersistenceError> {
    let recorded_at = u64::try_from(recorded_at)
        .map_err(|_| RestrictedIdentityLinkagePersistenceError::InvalidTimestamp)?;
    RestrictedIdentityLinkage::new(
        linkage_ref,
        participant_ref,
        research_participant_ref,
        research_program_ref,
        linkage_key_version,
        recorded_at,
    )
    .map_err(|_| RestrictedIdentityLinkagePersistenceError::ConflictingReplay)
}

fn required_reference(reference: &str) -> Result<&str, RestrictedIdentityLinkagePersistenceError> {
    if reference.trim() != reference {
        return Err(RestrictedIdentityLinkagePersistenceError::InvalidReference);
    }
    normalized_reference(reference)
        .ok_or(RestrictedIdentityLinkagePersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, RestrictedIdentityLinkagePersistenceError> {
    i64::try_from(timestamp)
        .map_err(|_| RestrictedIdentityLinkagePersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), RestrictedIdentityLinkagePersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(RestrictedIdentityLinkagePersistenceError::UnsupportedIsolationLevel)
    }
}

fn is_unique_violation(error: &postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| code == &SqlState::UNIQUE_VIOLATION)
}

#[cfg(test)]
mod tests {
    use super::{
        postgres_timestamp, reconstruct_stored_linkage, required_reference,
        RestrictedIdentityLinkagePersistenceError,
    };
    use std::error::Error;

    #[test]
    fn persistence_errors_and_guards_cover_fail_closed_arms() {
        for (error, expected) in [
            (
                RestrictedIdentityLinkagePersistenceError::InvalidReference,
                "restricted linkage persistence references must be opaque values",
            ),
            (
                RestrictedIdentityLinkagePersistenceError::ConflictingReplay,
                "restricted linkage identity was replayed with conflicting evidence",
            ),
            (
                RestrictedIdentityLinkagePersistenceError::InvalidTimestamp,
                "restricted linkage timestamp exceeds the PostgreSQL bigint range",
            ),
            (
                RestrictedIdentityLinkagePersistenceError::UnsupportedIsolationLevel,
                "restricted linkage persistence requires read committed isolation",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(error.source().is_none());
        }
        assert!(matches!(
            required_reference(" "),
            Err(RestrictedIdentityLinkagePersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(RestrictedIdentityLinkagePersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference(" linkage_commons_program_one"),
            Err(RestrictedIdentityLinkagePersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("linkage_commons_program_one").unwrap(),
            "linkage_commons_program_one"
        );
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(RestrictedIdentityLinkagePersistenceError::InvalidTimestamp)
        ));
        assert_eq!(
            postgres_timestamp(1_724_000_000_000).unwrap(),
            1_724_000_000_000
        );
        assert!(matches!(
            reconstruct_stored_linkage(
                "linkage_commons_program_one",
                "participant_operational_one",
                "participant_operational_one",
                "research_program_commons_one",
                "linkage_key_version_2026_q3",
                1_724_000_000_000,
            ),
            Err(RestrictedIdentityLinkagePersistenceError::ConflictingReplay)
        ));
        assert!(matches!(
            reconstruct_stored_linkage(
                "linkage_commons_program_one",
                "participant_operational_one",
                "research_participant_program_one",
                "research_program_commons_one",
                "linkage_key_version_2026_q3",
                -1,
            ),
            Err(RestrictedIdentityLinkagePersistenceError::InvalidTimestamp)
        ));
        let reconstructed = reconstruct_stored_linkage(
            "linkage_commons_program_one",
            "participant_operational_one",
            "research_participant_program_one",
            "research_program_commons_one",
            "linkage_key_version_2026_q3",
            1_724_000_000_000,
        )
        .unwrap();
        assert_eq!(
            reconstructed.research_participant_ref(),
            "research_participant_program_one"
        );
    }
}
