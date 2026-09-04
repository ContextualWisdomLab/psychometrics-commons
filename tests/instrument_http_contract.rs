//! Public instrument catalog HTTP contract for startable published releases.
//!
//! A purchaser lists the exact locale-specific releases that may begin a new
//! session, then uses `release_ref` and `locale` with `POST /v1/sessions`.
//! Draft, suspended, and retired releases stay hidden so unpublished catalog
//! rows cannot be discovered through this family.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::instrument_http::{
    accept_one_instrument_http, bind_instrument_http, handle_instrument_http_request,
    InstrumentHttpRuntime, INSTRUMENT_COLLECTION_PATH,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ENGLISH_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const ENGLISH_EVIDENCE_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn manifest(
    release_ref: &str,
    instrument_ref: &str,
    locale: &str,
    digest: &str,
) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        instrument_ref,
        "instrument_version_big_five_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        locale,
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_version_big_five_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        digest,
    )
    .unwrap()
}

fn approved_evidence(
    evidence_ref: &str,
    release_ref: &str,
    locale: &str,
    digest: &str,
    evidence_digest: &str,
) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        evidence_ref,
        "evidence_policy_self_reflection_v1",
        release_ref,
        "instrument_version_big_five_v1",
        &["item_version_001", "item_version_002"],
        digest,
        locale,
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_version_big_five_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            evidence_digest,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_v1"],
        &["approval_psychometrics_big_five_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap()
}

fn publish(
    mut release: InstrumentRelease,
    evidence: PublicationEvidenceRecord,
    reviewed_at_unix_ms: u64,
    published_at_unix_ms: u64,
) -> InstrumentRelease {
    release
        .apply_command(
            "publication_review_catalog",
            PublicationCommand::SubmitReview,
            reviewed_at_unix_ms,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_catalog",
            PublicationCommand::Publish,
            published_at_unix_ms,
        )
        .unwrap();
    release
}

fn published_korean() -> InstrumentRelease {
    publish(
        InstrumentRelease::new(
            manifest(
                "release_big_five_ko_v1",
                "instrument_big_five",
                "ko-KR",
                VALID_DIGEST,
            ),
            10_000,
        )
        .unwrap(),
        approved_evidence(
            "publication_evidence_big_five_ko_v1",
            "release_big_five_ko_v1",
            "ko-KR",
            VALID_DIGEST,
            EVIDENCE_DIGEST,
        ),
        10_100,
        10_200,
    )
}

fn published_english() -> InstrumentRelease {
    publish(
        InstrumentRelease::new(
            manifest(
                "release_big_five_en_v1",
                "instrument_big_five",
                "en-US",
                ENGLISH_DIGEST,
            ),
            11_000,
        )
        .unwrap(),
        approved_evidence(
            "publication_evidence_big_five_en_v1",
            "release_big_five_en_v1",
            "en-US",
            ENGLISH_DIGEST,
            ENGLISH_EVIDENCE_DIGEST,
        ),
        11_100,
        11_200,
    )
}

fn draft_korean() -> InstrumentRelease {
    InstrumentRelease::new(
        manifest(
            "release_big_five_ko_draft",
            "instrument_big_five",
            "ko-KR",
            VALID_DIGEST,
        ),
        10_000,
    )
    .unwrap()
}

fn get_request(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
}

#[test]
fn list_returns_only_published_releases_in_stable_order() {
    let runtime = InstrumentHttpRuntime::new(vec![
        draft_korean(),
        published_english(),
        published_korean(),
    ]);
    let response =
        handle_instrument_http_request(&get_request(INSTRUMENT_COLLECTION_PATH), &runtime);

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "application/json");
    let body = response.body();
    assert!(body.contains("\"release_ref\":\"release_big_five_en_v1\""));
    assert!(body.contains("\"release_ref\":\"release_big_five_ko_v1\""));
    assert!(body.contains("\"locale\":\"en-US\""));
    assert!(body.contains("\"locale\":\"ko-KR\""));
    assert!(body.contains("\"instrument_ref\":\"instrument_big_five\""));
    assert!(body.contains(&format!("\"content_digest\":\"{VALID_DIGEST}\"")));
    assert!(body.contains("\"state\":\"published\""));
    assert!(
        !body.contains("release_big_five_ko_draft"),
        "unpublished releases must stay hidden from the public catalog"
    );
    let english = body
        .find("release_big_five_en_v1")
        .expect("english release");
    let korean = body.find("release_big_five_ko_v1").expect("korean release");
    assert!(
        english < korean,
        "catalog order must be locale then release_ref so a purchaser can pick ko-KR or en-US predictably"
    );
}

#[test]
fn get_family_returns_published_releases_and_hides_unknown_or_draft_only_families() {
    let runtime = InstrumentHttpRuntime::new(vec![published_korean(), draft_korean()]);
    let found = handle_instrument_http_request(
        &get_request("/v1/instruments/instrument_big_five"),
        &runtime,
    );
    assert_eq!(found.status(), 200);
    assert!(found
        .body()
        .contains("\"instrument_ref\":\"instrument_big_five\""));
    assert!(found
        .body()
        .contains("\"release_ref\":\"release_big_five_ko_v1\""));
    assert!(!found.body().contains("release_big_five_ko_draft"));

    let missing = handle_instrument_http_request(
        &get_request("/v1/instruments/instrument_missing_family"),
        &runtime,
    );
    assert_eq!(missing.status(), 404);
    assert_eq!(missing.content_type(), "application/problem+json");
    assert!(missing
        .body()
        .contains("urn:psychometrics-commons:problem:instrument-not-found"));
    assert!(missing
        .body()
        .contains("Use GET /v1/instruments to list startable published releases"));
}

#[test]
fn invalid_paths_and_methods_fail_closed_without_leaking_catalog_rows() {
    let runtime = InstrumentHttpRuntime::new(vec![published_korean()]);

    let numeric = handle_instrument_http_request(&get_request("/v1/instruments/42"), &runtime);
    assert_eq!(numeric.status(), 400);
    assert!(numeric
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));

    let padded = handle_instrument_http_request(
        &get_request("/v1/instruments/%20instrument_big_five"),
        &runtime,
    );
    assert_eq!(padded.status(), 400);

    let post = handle_instrument_http_request(
        "POST /v1/instruments HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &runtime,
    );
    assert_eq!(post.status(), 405);
    assert!(post
        .body()
        .contains("urn:psychometrics-commons:problem:method-not-allowed"));

    let other = handle_instrument_http_request(&get_request("/v1/sessions"), &runtime);
    assert_eq!(other.status(), 404);

    let nested = handle_instrument_http_request(
        &get_request("/v1/instruments/instrument_big_five/extra"),
        &runtime,
    );
    assert_eq!(nested.status(), 404);

    let put_family = handle_instrument_http_request(
        "PUT /v1/instruments/instrument_big_five HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &runtime,
    );
    assert_eq!(put_family.status(), 405);

    let malformed = handle_instrument_http_request("not-an-http-request", &runtime);
    assert_eq!(malformed.status(), 400);
    assert_eq!(runtime.catalog_count(), 1);
}

#[test]
fn empty_catalog_and_unrelated_methods_stay_safe() {
    let empty = InstrumentHttpRuntime::new(Vec::new());
    let listed = handle_instrument_http_request(&get_request(INSTRUMENT_COLLECTION_PATH), &empty);
    assert_eq!(listed.status(), 200);
    assert_eq!(listed.body(), "{\"releases\":[]}");

    let queried =
        handle_instrument_http_request(&get_request("/v1/instruments?locale=ko-KR"), &empty);
    assert_eq!(queried.status(), 200);
    assert_eq!(queried.body(), "{\"releases\":[]}");

    let unrelated = handle_instrument_http_request(
        "POST /v1/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &empty,
    );
    assert_eq!(unrelated.status(), 404);
}

#[test]
fn listener_serves_one_published_catalog_request() {
    let runtime = InstrumentHttpRuntime::new(vec![published_korean()]);
    let listener = bind_instrument_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || accept_one_instrument_http(&listener, &runtime));

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(b"GET /v1/instruments HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut body = String::new();
    client.read_to_string(&mut body).unwrap();
    server.join().unwrap().unwrap();

    assert!(body.starts_with("HTTP/1.1 200 OK"));
    assert!(body.contains("release_big_five_ko_v1"));
    assert!(body.contains("application/json"));
}

#[test]
fn listener_fails_closed_for_truncated_and_extra_token_requests() {
    let runtime = InstrumentHttpRuntime::new(vec![published_korean()]);
    let listener = bind_instrument_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn({
        let runtime = InstrumentHttpRuntime::new(vec![published_korean()]);
        move || {
            accept_one_instrument_http(&listener, &runtime).unwrap();
            accept_one_instrument_http(&listener, &runtime).unwrap();
        }
    });

    let mut truncated = TcpStream::connect(address).unwrap();
    truncated
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    truncated.write_all(b"GET /v1/instruments").unwrap();
    truncated.shutdown(std::net::Shutdown::Write).unwrap();
    let mut truncated_body = String::new();
    truncated.read_to_string(&mut truncated_body).unwrap();
    assert!(
        truncated_body.starts_with("HTTP/1.1 400 Bad Request"),
        "{truncated_body}"
    );

    let extra = handle_instrument_http_request(
        "GET /v1/instruments HTTP/1.1 leftover\r\nHost: localhost\r\n\r\n",
        &runtime,
    );
    assert_eq!(extra.status(), 400);

    let mut extra_wire = TcpStream::connect(address).unwrap();
    extra_wire
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    extra_wire
        .write_all(b"GET /v1/instruments HTTP/1.1 leftover\r\nHost: localhost\r\n\r\n")
        .unwrap();
    extra_wire.shutdown(std::net::Shutdown::Write).unwrap();
    let mut extra_body = String::new();
    extra_wire.read_to_string(&mut extra_body).unwrap();
    assert!(
        extra_body.starts_with("HTTP/1.1 400 Bad Request"),
        "{extra_body}"
    );

    server.join().unwrap();
}
