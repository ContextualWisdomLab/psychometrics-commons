//! Fail-closed authorization coverage for non-canonical anonymous-credential bindings.

use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn credential() -> AnonymousCredential {
    AnonymousCredential::new(
        "anonymous_credential_coverage",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        DIGEST,
        1_000,
        2_000,
    )
    .unwrap()
}

#[test]
fn every_bound_reference_must_use_its_exact_canonical_spelling() {
    let credential = credential();

    for (tenant_ref, participant_ref, session_ref) in [
        (" tenant_alpha ", "participant_alpha", "session_alpha"),
        ("tenant_alpha", " participant_alpha ", "session_alpha"),
        ("tenant_alpha", "participant_alpha", " session_alpha "),
    ] {
        assert!(!credential.authorizes(DIGEST, tenant_ref, participant_ref, session_ref, 1_500,));
    }
}

#[test]
fn noncanonical_or_wrong_length_digest_never_reaches_authorized_outcome() {
    let credential = credential();

    for digest in [
        "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(!credential.authorizes(
            digest,
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            1_500,
        ));
    }
}
