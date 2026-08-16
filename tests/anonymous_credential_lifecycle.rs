//! Anonymous assessment credentials are short-lived, exact-bound, and revocable.

use psychometrics_commons_runtime::anonymous_credential::{
    AnonymousCredential, AnonymousCredentialError,
};
use std::error::Error;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn credential() -> AnonymousCredential {
    AnonymousCredential::new(
        "anonymous_credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        DIGEST_A,
        1_000,
        2_000,
    )
    .unwrap()
}

#[test]
fn exact_binding_and_digest_authorize_only_inside_the_server_window() {
    let credential = credential();

    assert_eq!(credential.credential_ref(), "anonymous_credential_alpha");
    assert_eq!(credential.tenant_ref(), "tenant_alpha");
    assert_eq!(credential.participant_ref(), "participant_alpha");
    assert_eq!(credential.session_ref(), "session_alpha");
    assert_eq!(credential.proof_digest(), DIGEST_A);
    assert_eq!(credential.issued_at_unix_ms(), 1_000);
    assert_eq!(credential.expires_at_unix_ms(), 2_000);
    assert_eq!(credential.revoked_at_unix_ms(), None);

    assert!(credential.is_valid_at(1_000));
    assert!(credential.is_valid_at(1_999));
    assert!(!credential.is_valid_at(0));
    assert!(!credential.is_valid_at(999));
    assert!(!credential.is_valid_at(2_000));

    assert!(credential.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));
    assert!(!credential.authorizes(
        DIGEST_B,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));
    assert!(!credential.authorizes(
        " sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));
    assert!(!credential.authorizes(
        DIGEST_A,
        "tenant_other",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));
    assert!(!credential.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_other",
        "session_alpha",
        1_500,
    ));
    assert!(!credential.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_other",
        1_500,
    ));
    assert!(!credential.authorizes(
        DIGEST_A,
        " tenant_alpha ",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));
    assert!(!credential.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        2_000,
    ));
}

#[test]
fn construction_rejects_malformed_identity_digest_and_lifetime_evidence() {
    let valid = [
        "anonymous_credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
    ];
    for index in 0..valid.len() {
        let mut references = valid;
        references[index] = "12345";
        assert_eq!(
            AnonymousCredential::new(
                references[0],
                references[1],
                references[2],
                references[3],
                DIGEST_A,
                1_000,
                2_000,
            )
            .unwrap_err(),
            AnonymousCredentialError::InvalidReference
        );
    }

    let noncanonical_reference_sets = [
        [
            " anonymous_credential_alpha ",
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
        ],
        [
            "anonymous_credential_alpha",
            " tenant_alpha ",
            "participant_alpha",
            "session_alpha",
        ],
        [
            "anonymous_credential_alpha",
            "tenant_alpha",
            "participant_alpha\t",
            "session_alpha",
        ],
        [
            "anonymous_credential_alpha",
            "tenant_alpha",
            "participant_alpha",
            "\u{00a0}session_alpha",
        ],
    ];
    for references in noncanonical_reference_sets {
        assert_eq!(
            AnonymousCredential::new(
                references[0],
                references[1],
                references[2],
                references[3],
                DIGEST_A,
                1_000,
                2_000,
            )
            .unwrap_err(),
            AnonymousCredentialError::InvalidReference,
            "credential construction must not normalize resource-identity aliases"
        );
    }

    for invalid_digest in [
        "",
        "sha256:abc",
        "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "sha256:gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
    ] {
        assert_eq!(
            AnonymousCredential::new(
                valid[0],
                valid[1],
                valid[2],
                valid[3],
                invalid_digest,
                1_000,
                2_000,
            )
            .unwrap_err(),
            AnonymousCredentialError::InvalidDigest
        );
    }

    for (issued_at, expires_at, expected) in [
        (0, 2_000, AnonymousCredentialError::InvalidTimestamp),
        (1_000, 0, AnonymousCredentialError::InvalidTimestamp),
        (1_000, 1_000, AnonymousCredentialError::InvalidLifetime),
        (2_000, 1_000, AnonymousCredentialError::InvalidLifetime),
    ] {
        assert_eq!(
            AnonymousCredential::new(
                valid[0], valid[1], valid[2], valid[3], DIGEST_A, issued_at, expires_at,
            )
            .unwrap_err(),
            expected
        );
    }
}

#[test]
fn revocation_is_immediate_and_exact_replay_is_idempotent() {
    let mut credential = credential();

    assert_eq!(
        credential.revoke(0).unwrap_err(),
        AnonymousCredentialError::InvalidTimestamp
    );
    assert_eq!(
        credential.revoke(999).unwrap_err(),
        AnonymousCredentialError::InvalidTimestamp
    );
    assert!(credential.revoke(1_500).is_ok());
    assert_eq!(credential.revoked_at_unix_ms(), Some(1_500));
    assert!(credential.is_valid_at(1_499));
    assert!(!credential.is_valid_at(1_500));
    assert!(!credential.authorizes(
        DIGEST_A,
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500,
    ));

    assert!(credential.revoke(1_500).is_ok());
    assert_eq!(
        credential.revoke(1_501).unwrap_err(),
        AnonymousCredentialError::ConflictingRevocation
    );
    assert_eq!(credential.revoked_at_unix_ms(), Some(1_500));
}

#[test]
fn errors_are_stable_beginner_readable_and_have_no_hidden_source() {
    let cases = [
        AnonymousCredentialError::InvalidReference,
        AnonymousCredentialError::InvalidDigest,
        AnonymousCredentialError::InvalidTimestamp,
        AnonymousCredentialError::InvalidLifetime,
        AnonymousCredentialError::ConflictingRevocation,
        AnonymousCredentialError::Unauthorized,
    ];

    for error in cases {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_none());
    }
}
