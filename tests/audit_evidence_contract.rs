//! Contract tests for immutable, purpose-bound product audit evidence.

use psychometrics_commons_runtime::audit::{
    AuditEvidence, AuditEvidenceError, AuditEvidenceInput, AuditOutcome,
};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn input<'a>(
    event_ref: &'a str,
    tenant_ref: &'a str,
    actor_ref: &'a str,
    purpose_code: &'a str,
    action_code: &'a str,
    resource_ref: &'a str,
    occurred_at_unix_ms: u64,
) -> AuditEvidenceInput<'a> {
    AuditEvidenceInput {
        audit_event_ref: event_ref,
        tenant_ref,
        actor_ref,
        purpose_code,
        action_code,
        resource_ref,
        outcome: AuditOutcome::Succeeded,
        evidence_digest: DIGEST,
        occurred_at_unix_ms,
    }
}

#[test]
fn privileged_product_action_keeps_exact_purpose_actor_resource_and_digest() {
    let evidence = AuditEvidence::new(input(
        "audit_event_publish_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        "instrument_publication",
        "publish_instrument_release",
        "instrument_release_big_five_ko_v1",
        1_785_000_000_000,
    ))
    .unwrap();

    assert_eq!(evidence.audit_event_ref(), "audit_event_publish_01");
    assert_eq!(evidence.tenant_ref(), "tenant_research_alpha");
    assert_eq!(evidence.actor_ref(), "actor_publisher_alpha");
    assert_eq!(evidence.purpose_code(), "instrument_publication");
    assert_eq!(evidence.action_code(), "publish_instrument_release");
    assert_eq!(evidence.resource_ref(), "instrument_release_big_five_ko_v1");
    assert_eq!(evidence.outcome(), AuditOutcome::Succeeded);
    assert_eq!(evidence.outcome().as_code(), "succeeded");
    assert_eq!(evidence.evidence_digest(), DIGEST);
    assert_eq!(evidence.occurred_at_unix_ms(), 1_785_000_000_000);
}

#[test]
fn denied_action_is_still_durable_audit_evidence_without_sensitive_payload() {
    let mut denied = input(
        "audit_event_denial_01",
        "tenant_research_alpha",
        "actor_researcher_alpha",
        "research_release_access",
        "read_restricted_linkage",
        "research_linkage_alpha",
        1_785_000_000_001,
    );
    denied.outcome = AuditOutcome::Denied;
    let evidence = AuditEvidence::new(denied).unwrap();

    assert_eq!(evidence.outcome(), AuditOutcome::Denied);
    assert_eq!(evidence.outcome().as_code(), "denied");
    assert!(!evidence.evidence_digest().contains("response"));
}

#[test]
fn identity_aliases_numeric_ids_controls_and_invisible_references_fail_closed() {
    for invalid in [
        "",
        " ",
        " audit_event_alias ",
        "12345",
        "+12.3e4",
        "audit_event_\u{0001}_alias",
        "audit_event_\u{200b}_alias",
        "audit_event_\u{2060}_alias",
    ] {
        let error = AuditEvidence::new(input(
            invalid,
            "tenant_research_alpha",
            "actor_publisher_alpha",
            "instrument_publication",
            "publish_instrument_release",
            "instrument_release_big_five_ko_v1",
            1_785_000_000_000,
        ))
        .unwrap_err();
        assert_eq!(error, AuditEvidenceError::InvalidReference);
    }
}

#[test]
fn purpose_and_action_codes_may_include_digits_after_a_leading_letter() {
    let evidence = AuditEvidence::new(input(
        "audit_event_code_digit_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        "instrument_publication_v2",
        "publish_instrument_release_v3",
        "instrument_release_big_five_ko_v1",
        1_785_000_000_000,
    ))
    .expect("lowercase machine tokens may include digits after the leading letter");

    assert_eq!(evidence.purpose_code(), "instrument_publication_v2");
    assert_eq!(evidence.action_code(), "publish_instrument_release_v3");
}

#[test]
fn purpose_and_action_codes_are_stable_lowercase_ascii_tokens() {
    for invalid_code in [
        "",
        "Instrument_Publication",
        "has-hyphen",
        "has space",
        "å",
        "2instrument_publication",
    ] {
        let mut invalid_purpose = input(
            "audit_event_code_01",
            "tenant_research_alpha",
            "actor_publisher_alpha",
            invalid_code,
            "publish_instrument_release",
            "instrument_release_big_five_ko_v1",
            1_785_000_000_000,
        );
        assert_eq!(
            AuditEvidence::new(invalid_purpose).unwrap_err(),
            AuditEvidenceError::InvalidCode
        );

        invalid_purpose = input(
            "audit_event_code_02",
            "tenant_research_alpha",
            "actor_publisher_alpha",
            "instrument_publication",
            invalid_code,
            "instrument_release_big_five_ko_v1",
            1_785_000_000_000,
        );
        assert_eq!(
            AuditEvidence::new(invalid_purpose).unwrap_err(),
            AuditEvidenceError::InvalidCode
        );
    }
}

#[test]
fn digest_and_server_time_fail_closed() {
    for invalid_digest in [
        "",
        "sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        "sha256:deadbeef",
        "md5:0123456789abcdef0123456789abcdef",
    ] {
        let mut invalid = input(
            "audit_event_digest_01",
            "tenant_research_alpha",
            "actor_publisher_alpha",
            "instrument_publication",
            "publish_instrument_release",
            "instrument_release_big_five_ko_v1",
            1_785_000_000_000,
        );
        invalid.evidence_digest = invalid_digest;
        assert_eq!(
            AuditEvidence::new(invalid).unwrap_err(),
            AuditEvidenceError::InvalidDigest
        );
    }

    assert_eq!(
        AuditEvidence::new(input(
            "audit_event_time_01",
            "tenant_research_alpha",
            "actor_publisher_alpha",
            "instrument_publication",
            "publish_instrument_release",
            "instrument_release_big_five_ko_v1",
            0,
        ))
        .unwrap_err(),
        AuditEvidenceError::InvalidTimestamp
    );
}
