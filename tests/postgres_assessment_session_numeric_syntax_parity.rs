//! PostgreSQL/Rust parity for mixed numeric-like assessment-session references.
//!
//! Single-character Unicode sweeps prove the numeric code-point set. This contract covers the
//! second half of the rule: once a reference contains a numeric character, only the exact
//! separator/exponent characters accepted by the shared Rust boundary may keep it numeric-like.
//! The database must reject those spellings and preserve identifiers that contain any other
//! visible identity material, independent of caller collation.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_assessment_session::apply_assessment_session_migration;
use psychometrics_commons_runtime::session::AssessmentSession;

const CONTENT_DIGEST: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn rust_accepts_reference(reference: &str) -> bool {
    AssessmentSession::from_persisted_created(
        reference,
        "participant_ref_alpha",
        "instrument_release_alpha",
        "instrument_version_alpha",
        CONTENT_DIGEST,
        "en-US",
        1,
    )
    .is_ok()
}

#[test]
fn postgres_numeric_syntax_additional_set_matches_the_real_rust_boundary() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema = format!(
        "assessment_session_numeric_syntax_parity_test_{}",
        std::process::id()
    );
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;\
             CREATE SCHEMA {schema};\
             SET search_path TO {schema};\
             CREATE COLLATION {schema}.numeric_syntax_fold (\
                 provider = icu, locale = 'und-u-ks-level1', deterministic = false\
             );"
        ))
        .expect("isolated numeric-syntax parity schema and ICU collation must be created");
    apply_assessment_session_migration(&mut client).unwrap();

    let samples = [
        "+1",
        "-1",
        "1.5",
        "1,5",
        "1e5",
        "1E-5",
        "١٫٥",
        "١٬٥",
        "１２．５",
        "１２，５",
        "ⅣE2",
        "job+1",
        "1:5",
        "number_١٫٥",
        "version１２．５",
    ];

    for sample in samples {
        let postgres_accepts: bool = client
            .query_one(
                "SELECT assessment_session_reference_is_valid($1)",
                &[&sample],
            )
            .expect("PostgreSQL numeric-syntax parity query must execute")
            .get(0);
        assert_eq!(
            postgres_accepts,
            rust_accepts_reference(sample),
            "PostgreSQL/Rust numeric-like reference drift for {sample:?}"
        );
    }

    // A collatable SQL function inherits the call's collation. These references are visible,
    // non-numeric Rust identities even though an accent-insensitive ICU collation considers their
    // final scalar equal to the ASCII exponent marker `e`. Separator membership must therefore be
    // code-point exact rather than text-collation equality.
    for sample in ["1é", "1É"] {
        assert!(rust_accepts_reference(sample));
        let query = format!(
            "SELECT assessment_session_reference_is_valid($1::text COLLATE {schema}.numeric_syntax_fold)"
        );
        let postgres_accepts: bool = client
            .query_one(&query, &[&sample])
            .expect("PostgreSQL nondeterministic-collation parity query must execute")
            .get(0);
        assert!(
            postgres_accepts,
            "PostgreSQL treated visible identity material as a numeric separator under a nondeterministic collation: {sample:?}"
        );
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE;"))
        .expect("isolated numeric-syntax parity schema must be removable");
}
