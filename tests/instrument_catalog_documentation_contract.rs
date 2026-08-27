const CATALOG_SOURCE: &str = include_str!("../src/postgres_instrument_catalog.rs");
const ARCHITECTURE: &str = include_str!("../ARCHITECTURE.md");

#[test]
fn public_catalog_docs_explain_pagination_and_revalidation_in_plain_language() {
    for required_phrase in [
        "cursor resumes immediately after the last returned row",
        "non-locking catalog read",
        "publication state can change after the catalog read",
        "locks and revalidates the exact release",
    ] {
        assert!(
            CATALOG_SOURCE.contains(required_phrase),
            "public catalog documentation must explain `{required_phrase}`"
        );
    }
}

#[test]
fn architecture_keeps_catalog_discovery_advisory_and_lifecycle_owned() {
    for required_phrase in [
        "Instrument catalog discovery is an advisory read",
        "does not add a publication lifecycle state",
        "session start locks and revalidates the exact persisted release",
    ] {
        assert!(
            ARCHITECTURE.contains(required_phrase),
            "architecture must record catalog boundary `{required_phrase}`"
        );
    }
}
