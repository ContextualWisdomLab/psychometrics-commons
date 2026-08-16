//! Created-session reconstitution without a currently published release.

use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionReconstitutionError, SessionState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SESSION_REF: &str = "ses_9c2e1a0b4d5f67890123456789abcdef";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const RELEASE_REF: &str = "release_big_five_ko_v1";
const VERSION_REF: &str = "instrument_version_big_five_ko_v1";

#[test]
fn persisted_created_identity_restores_without_a_live_release() {
    let mut session = AssessmentSession::from_persisted_created(
        SESSION_REF,
        PARTICIPANT_REF,
        RELEASE_REF,
        VERSION_REF,
        VALID_DIGEST,
        "ko-KR",
        20_000,
    )
    .unwrap();

    assert_eq!(session.session_ref(), SESSION_REF);
    assert_eq!(session.participant_ref(), PARTICIPANT_REF);
    assert_eq!(session.instrument_release_ref(), RELEASE_REF);
    assert_eq!(session.instrument_version_ref(), VERSION_REF);
    assert_eq!(session.instrument_release_content_digest(), VALID_DIGEST);
    assert_eq!(session.locale(), "ko-KR");
    assert_eq!(session.created_at_unix_ms(), 20_000);
    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(
        session
            .apply_command(
                "cmd_activate_persisted_session",
                1,
                SessionCommand::Activate
            )
            .unwrap(),
        SessionState::Active
    );
}

#[derive(Clone, Copy)]
struct StoredCreatedIdentity<'a> {
    session_ref: &'a str,
    participant_ref: &'a str,
    instrument_release_ref: &'a str,
    instrument_version_ref: &'a str,
    digest: &'a str,
    locale: &'a str,
    created_at_unix_ms: u64,
}

fn valid_stored_identity() -> StoredCreatedIdentity<'static> {
    StoredCreatedIdentity {
        session_ref: SESSION_REF,
        participant_ref: PARTICIPANT_REF,
        instrument_release_ref: RELEASE_REF,
        instrument_version_ref: VERSION_REF,
        digest: VALID_DIGEST,
        locale: "ko-KR",
        created_at_unix_ms: 20_000,
    }
}

fn assert_reconstitution_error(
    identity: StoredCreatedIdentity<'_>,
    expected: SessionReconstitutionError,
) {
    assert_eq!(
        AssessmentSession::from_persisted_created(
            identity.session_ref,
            identity.participant_ref,
            identity.instrument_release_ref,
            identity.instrument_version_ref,
            identity.digest,
            identity.locale,
            identity.created_at_unix_ms,
        ),
        Err(expected)
    );
}

#[test]
fn persisted_created_identity_rejects_numeric_or_blank_references() {
    let mut identity = valid_stored_identity();
    identity.session_ref = "12345";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);

    identity = valid_stored_identity();
    identity.participant_ref = "12345";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);

    identity = valid_stored_identity();
    identity.participant_ref = " ";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);

    identity = valid_stored_identity();
    identity.instrument_release_ref = "12345";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);

    identity = valid_stored_identity();
    identity.instrument_release_ref = "";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);

    identity = valid_stored_identity();
    identity.instrument_version_ref = "12345";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);

    identity = valid_stored_identity();
    identity.instrument_version_ref = " ";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidReference);
}

#[test]
fn persisted_created_identity_rejects_invalid_digest_locale_and_timestamp() {
    let mut identity = valid_stored_identity();
    identity.digest = "sha256:not-a-digest";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidContentDigest);

    identity = valid_stored_identity();
    identity.locale = " ko-KR";
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidLocale);

    identity = valid_stored_identity();
    identity.created_at_unix_ms = 0;
    assert_reconstitution_error(identity, SessionReconstitutionError::InvalidTimestamp);
}

#[test]
fn persisted_identity_errors_tell_the_caller_what_to_fix() {
    assert_eq!(
        SessionReconstitutionError::InvalidReference.to_string(),
        "use an opaque non-numeric session, participant, release, or version reference"
    );
    assert_eq!(
        SessionReconstitutionError::InvalidTimestamp.to_string(),
        "use a stored creation time greater than zero"
    );
    assert_eq!(
        SessionReconstitutionError::InvalidContentDigest.to_string(),
        "use a sha256 digest with 64 lowercase hexadecimal digits"
    );
    assert_eq!(
        SessionReconstitutionError::InvalidLocale.to_string(),
        "use an exact whitespace-free BCP 47-style locale tag"
    );
}
