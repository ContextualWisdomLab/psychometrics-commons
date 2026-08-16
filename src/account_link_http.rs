//! Public HTTP transport for dual-proof account-link persist, recover, and unlink.
//!
//! This slice exposes `POST /v1/account-links`,
//! `POST /v1/account-links/recover`, and `POST /v1/account-links/unlink`
//! over HTTP/1.1. Adapters validate anonymous-session and Keyverse proofs
//! before constructing the JSON body. The handler does not parse tokens. It
//! uses the server clock as `linked_at_unix_ms`, recover `now`, and unlink
//! `ended_at_unix_ms`, then calls
//! [`crate::account_link_write::persist_authorized_account_link`],
//! [`crate::account_link_write::recover_participant_for_authenticated_account`],
//! and [`crate::account_link_write::persist_authorized_account_unlink`].
//! Unlink recovers from the current proof first and never treats a client
//! `participant_ref` as a capability grant. Errors use RFC 9457 problem
//! details and never echo raw request bodies, SQL, or provider text.

use crate::account_link::{AccountLinkAuthorizationError, AuthenticatedAccountControl};
use crate::account_link_write::{
    persist_authorized_account_link, persist_authorized_account_unlink,
    recover_participant_for_authenticated_account, AccountLinkWriteError,
};
use crate::anonymous_session::AnonymousSessionContext;
use crate::participant::{AccountLinkError, ParticipantRecord};
use crate::postgres_participant_identity_link::{
    load_participant_identity_history, IdentityLinkPersistenceDisposition,
    IdentityLinkPersistenceError,
};
use postgres::Transaction;
use std::collections::HashMap;
use std::fmt::Write;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Public collection path for account-link persist.
pub const ACCOUNT_LINK_COLLECTION_PATH: &str = "/v1/account-links";
/// Public command path for returning-account recover.
pub const ACCOUNT_LINK_RECOVER_PATH: &str = "/v1/account-links/recover";
/// Public command path for current-proof unlink.
pub const ACCOUNT_LINK_UNLINK_PATH: &str = "/v1/account-links/unlink";
/// Bounded read/write timeout for one accepted account-link HTTP connection.
pub const ACCOUNT_LINK_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted account-link HTTP request size, including headers and body.
pub const ACCOUNT_LINK_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

/// Server clock and process-local persist/unlink idempotency store.
pub struct AccountLinkHttpRuntime {
    now_unix_ms: u64,
    idempotency: HashMap<String, IdempotentPersist>,
}

struct IdempotentPersist {
    fingerprint: String,
    status: u16,
    body: String,
}

impl AccountLinkHttpRuntime {
    /// Create a runtime that uses one server-authoritative clock.
    ///
    /// `now_unix_ms` is the link, recover, and unlink time. Callers must not
    /// pass a client-supplied timestamp.
    #[must_use]
    pub fn new(now_unix_ms: u64) -> Self {
        Self {
            now_unix_ms,
            idempotency: HashMap::new(),
        }
    }

    /// Return the server clock used for persist, recover, and unlink.
    #[must_use]
    pub const fn now_unix_ms(&self) -> u64 {
        self.now_unix_ms
    }
}

/// HTTP response produced by a public account-link request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLinkHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl AccountLinkHttpResponse {
    fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json",
            body,
        }
    }

    fn problem(status: u16, type_uri: &str, title: &str, detail: &str) -> Self {
        Self {
            status,
            content_type: "application/problem+json",
            body: format!(
                "{{\"type\":{},\"title\":{},\"status\":{status},\"detail\":{}}}",
                json_string(type_uri),
                json_string(title),
                json_string(detail)
            ),
        }
    }

    /// Return the HTTP status code.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Return the response content type.
    #[must_use]
    pub const fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// Return the response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Dual-proof persist request accepted after HTTP classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLinkPersistRequest {
    idempotency_key: String,
    participant_ref: String,
    tenant_ref: String,
    anonymous_session_ref: String,
    anonymous_proof_ref: String,
    anonymous_valid_until_unix_ms: u64,
    identity_issuer: String,
    identity_subject_ref: String,
    authenticated_proof_ref: String,
    authenticated_valid_until_unix_ms: u64,
    link_event_ref: String,
}

impl AccountLinkPersistRequest {
    /// Return the opaque HTTP replay key.
    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    /// Return the product-owned participant reference.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the tenant that owns the participant.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the identity issuer for the authenticated subject.
    #[must_use]
    pub fn identity_issuer(&self) -> &str {
        &self.identity_issuer
    }

    /// Return the issuer-scoped authenticated subject.
    #[must_use]
    pub fn identity_subject_ref(&self) -> &str {
        &self.identity_subject_ref
    }

    /// Return the durable account-link event reference.
    #[must_use]
    pub fn link_event_ref(&self) -> &str {
        &self.link_event_ref
    }

    /// Return the anonymous-session validity boundary.
    #[must_use]
    pub const fn anonymous_valid_until_unix_ms(&self) -> u64 {
        self.anonymous_valid_until_unix_ms
    }

    /// Return the authenticated-account validity boundary.
    #[must_use]
    pub const fn authenticated_valid_until_unix_ms(&self) -> u64 {
        self.authenticated_valid_until_unix_ms
    }

    fn fingerprint(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.participant_ref,
            self.tenant_ref,
            self.anonymous_session_ref,
            self.anonymous_proof_ref,
            self.anonymous_valid_until_unix_ms,
            self.identity_issuer,
            self.identity_subject_ref,
            self.authenticated_proof_ref,
            self.authenticated_valid_until_unix_ms,
            self.link_event_ref,
            self.idempotency_key
        )
    }
}

/// Returning-account recover request accepted after HTTP classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLinkRecoverRequest {
    tenant_ref: String,
    identity_issuer: String,
    identity_subject_ref: String,
    authenticated_proof_ref: String,
    authenticated_valid_until_unix_ms: u64,
}

impl AccountLinkRecoverRequest {
    /// Return the tenant asserted by the validated account proof.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the identity issuer for the authenticated subject.
    #[must_use]
    pub fn identity_issuer(&self) -> &str {
        &self.identity_issuer
    }

    /// Return the issuer-scoped authenticated subject.
    #[must_use]
    pub fn identity_subject_ref(&self) -> &str {
        &self.identity_subject_ref
    }

    /// Return opaque authenticated proof evidence.
    #[must_use]
    pub fn authenticated_proof_ref(&self) -> &str {
        &self.authenticated_proof_ref
    }

    /// Return the authenticated-account validity boundary.
    #[must_use]
    pub const fn authenticated_valid_until_unix_ms(&self) -> u64 {
        self.authenticated_valid_until_unix_ms
    }
}

/// Current-proof unlink request accepted after HTTP classification.
///
/// The body carries only the still-valid Keyverse proof and the append-only
/// end-event identity. A client-supplied `participant_ref` is rejected so a
/// previously recovered identifier cannot be replayed as a capability grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLinkUnlinkRequest {
    idempotency_key: String,
    tenant_ref: String,
    identity_issuer: String,
    identity_subject_ref: String,
    authenticated_proof_ref: String,
    authenticated_valid_until_unix_ms: u64,
    link_end_event_ref: String,
}

impl AccountLinkUnlinkRequest {
    /// Return the opaque HTTP replay key.
    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    /// Return the tenant asserted by the validated account proof.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the identity issuer for the authenticated subject.
    #[must_use]
    pub fn identity_issuer(&self) -> &str {
        &self.identity_issuer
    }

    /// Return the issuer-scoped authenticated subject.
    #[must_use]
    pub fn identity_subject_ref(&self) -> &str {
        &self.identity_subject_ref
    }

    /// Return the durable unlink event reference.
    #[must_use]
    pub fn link_end_event_ref(&self) -> &str {
        &self.link_end_event_ref
    }

    /// Return the authenticated-account validity boundary.
    #[must_use]
    pub const fn authenticated_valid_until_unix_ms(&self) -> u64 {
        self.authenticated_valid_until_unix_ms
    }

    fn fingerprint(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.tenant_ref,
            self.identity_issuer,
            self.identity_subject_ref,
            self.authenticated_proof_ref,
            self.authenticated_valid_until_unix_ms,
            self.link_end_event_ref,
            self.idempotency_key
        )
    }
}

/// Result of classifying one raw HTTP/1.1 account-link request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountLinkHttpClassification {
    /// Persist both current proofs through the write command.
    Persist(AccountLinkPersistRequest),
    /// Recover the participant bound to a still-valid account proof.
    Recover(AccountLinkRecoverRequest),
    /// End the current binding after recovering from the same proof.
    Unlink(AccountLinkUnlinkRequest),
    /// Fail-closed HTTP response that must not touch the store.
    Ready(AccountLinkHttpResponse),
}

/// Translate one raw HTTP/1.1 request into persist, recover, unlink, or a problem.
///
/// Unknown methods, paths, JSON shapes, missing persist/unlink idempotency
/// keys, and a client-supplied unlink `participant_ref` fail closed with RFC
/// 9457 problem details and do not open a transaction.
#[must_use]
pub fn classify_account_link_http_request(request: &str) -> AccountLinkHttpClassification {
    let Some((method, target)) = parse_request_line(request) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link request must include an HTTP method and target",
        ));
    };
    let path = split_target(target).0;
    match (method, path) {
        ("POST", ACCOUNT_LINK_COLLECTION_PATH) => classify_persist(request),
        ("POST", ACCOUNT_LINK_RECOVER_PATH) => classify_recover(request),
        ("POST", ACCOUNT_LINK_UNLINK_PATH) => classify_unlink(request),
        (
            _,
            ACCOUNT_LINK_COLLECTION_PATH | ACCOUNT_LINK_RECOVER_PATH | ACCOUNT_LINK_UNLINK_PATH,
        ) => AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            405,
            "urn:psychometrics-commons:problem:method-not-allowed",
            "Method Not Allowed",
            "account-link routes accept POST /v1/account-links, POST /v1/account-links/recover, and POST /v1/account-links/unlink only",
        )),
        (_, path) if path.starts_with("/v1/account-links/") => {
            AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
                405,
                "urn:psychometrics-commons:problem:method-not-allowed",
                "Method Not Allowed",
                "account-link routes accept POST /v1/account-links, POST /v1/account-links/recover, and POST /v1/account-links/unlink only",
            ))
        }
        _ => AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "account-link routes accept POST /v1/account-links, POST /v1/account-links/recover, and POST /v1/account-links/unlink only",
        )),
    }
}

/// Authorize, persist, recover, or unlink, and return the HTTP response.
///
/// Persist loads or mints the anonymous participant, then calls the dual-proof
/// write command with the server clock. Recover and unlink use the same clock
/// and re-check the current binding from the authenticated proof. Unlink never
/// reads a client `participant_ref`.
#[must_use]
pub fn handle_account_link_http_request(
    request: &str,
    runtime: &mut AccountLinkHttpRuntime,
    transaction: &mut Transaction<'_>,
) -> AccountLinkHttpResponse {
    match classify_account_link_http_request(request) {
        AccountLinkHttpClassification::Ready(response) => response,
        AccountLinkHttpClassification::Persist(persist) => {
            execute_persist(runtime, transaction, &persist)
        }
        AccountLinkHttpClassification::Recover(recover) => {
            execute_recover(runtime, transaction, &recover)
        }
        AccountLinkHttpClassification::Unlink(unlink) => {
            execute_unlink(runtime, transaction, &unlink)
        }
    }
}

/// Bind a blocking TCP listener for public account-link HTTP.
///
/// Tests and local operators typically bind `127.0.0.1:0`. Hosted processes bind
/// `0.0.0.0:$PORT`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_account_link_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single account-link HTTP request.
///
/// The connection is closed after the response. The caller owns the client and
/// must supply a transaction that this function commits on success responses
/// and rolls back on problem responses.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_account_link_http(
    listener: &TcpListener,
    runtime: &mut AccountLinkHttpRuntime,
    transaction: &mut Transaction<'_>,
) -> io::Result<AccountLinkHttpResponse> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(ACCOUNT_LINK_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(ACCOUNT_LINK_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_account_link_http_request(&request, runtime, transaction);
    write_http_response(&mut stream, &response)?;
    Ok(response)
}

fn classify_persist(request: &str) -> AccountLinkHttpClassification {
    let Some(idempotency_key) =
        header_value(request, "idempotency-key").and_then(valid_idempotency_key)
    else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/account-links requires an opaque Idempotency-Key header",
        ));
    };
    let Some(body) = request_body(request) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link persist requires a JSON object body",
        ));
    };
    let Some(persist) = parse_persist_body(body, idempotency_key) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link persist requires participant, tenant, dual-proof, and link-event fields",
        ));
    };
    AccountLinkHttpClassification::Persist(persist)
}

fn classify_recover(request: &str) -> AccountLinkHttpClassification {
    let Some(body) = request_body(request) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link recover requires a JSON object body",
        ));
    };
    let Some(recover) = parse_recover_body(body) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link recover requires tenant and still-valid authenticated proof fields",
        ));
    };
    AccountLinkHttpClassification::Recover(recover)
}

fn classify_unlink(request: &str) -> AccountLinkHttpClassification {
    let Some(idempotency_key) =
        header_value(request, "idempotency-key").and_then(valid_idempotency_key)
    else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/account-links/unlink requires an opaque Idempotency-Key header",
        ));
    };
    let Some(body) = request_body(request) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link unlink requires a JSON object body",
        ));
    };
    let Some(unlink) = parse_unlink_body(body, idempotency_key) else {
        return AccountLinkHttpClassification::Ready(AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "account-link unlink requires tenant, still-valid authenticated proof, and link-end fields, and rejects participant_ref",
        ));
    };
    AccountLinkHttpClassification::Unlink(unlink)
}

fn execute_persist(
    runtime: &mut AccountLinkHttpRuntime,
    transaction: &mut Transaction<'_>,
    persist: &AccountLinkPersistRequest,
) -> AccountLinkHttpResponse {
    let fingerprint = persist.fingerprint();
    if let Some(existing) = runtime.idempotency.get(&persist.idempotency_key) {
        if existing.fingerprint == fingerprint {
            return AccountLinkHttpResponse::json(existing.status, existing.body.clone());
        }
        return AccountLinkHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key was reused with a different account-link persist body",
        );
    }
    let mut participant = match load_or_mint_participant(transaction, persist, runtime.now_unix_ms)
    {
        Ok(participant) => participant,
        Err(response) => return response,
    };
    let Ok(anonymous) = AnonymousSessionContext::new(
        &persist.tenant_ref,
        &persist.participant_ref,
        &persist.anonymous_session_ref,
        &persist.anonymous_proof_ref,
        persist.anonymous_valid_until_unix_ms,
    ) else {
        return AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-anonymous-proof",
            "Invalid Anonymous Proof",
            "anonymous-session proof fields must be opaque and currently valid",
        );
    };
    let Ok(authenticated) = AuthenticatedAccountControl::new(
        &persist.tenant_ref,
        &persist.identity_issuer,
        &persist.identity_subject_ref,
        &persist.authenticated_proof_ref,
        persist.authenticated_valid_until_unix_ms,
    ) else {
        return AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-authenticated-proof",
            "Invalid Authenticated Proof",
            "authenticated account-control fields must be opaque and currently valid",
        );
    };
    match persist_authorized_account_link(
        transaction,
        &mut participant,
        &anonymous,
        &authenticated,
        &persist.link_event_ref,
        runtime.now_unix_ms,
    ) {
        Ok(disposition) => {
            let (status, disposition_label) = match disposition {
                IdentityLinkPersistenceDisposition::Inserted => (201, "inserted"),
                IdentityLinkPersistenceDisposition::Duplicate => (200, "duplicate"),
            };
            let body = account_link_body(&participant, disposition_label);
            runtime.idempotency.insert(
                persist.idempotency_key.clone(),
                IdempotentPersist {
                    fingerprint,
                    status: if status == 201 { 200 } else { status },
                    body: body.clone(),
                },
            );
            AccountLinkHttpResponse::json(status, body)
        }
        Err(error) => map_account_link_write_error(&error),
    }
}

fn execute_recover(
    runtime: &AccountLinkHttpRuntime,
    transaction: &mut Transaction<'_>,
    recover: &AccountLinkRecoverRequest,
) -> AccountLinkHttpResponse {
    let Ok(authenticated) = AuthenticatedAccountControl::new(
        &recover.tenant_ref,
        &recover.identity_issuer,
        &recover.identity_subject_ref,
        &recover.authenticated_proof_ref,
        recover.authenticated_valid_until_unix_ms,
    ) else {
        return AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-authenticated-proof",
            "Invalid Authenticated Proof",
            "authenticated account-control fields must be opaque and currently valid",
        );
    };
    match recover_participant_for_authenticated_account(
        transaction,
        &authenticated,
        runtime.now_unix_ms,
    ) {
        Ok(Some(record)) => {
            AccountLinkHttpResponse::json(200, account_link_body(&record, "current"))
        }
        Ok(None) => AccountLinkHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:account-link-not-found",
            "Account Link Not Found",
            "a valid unused account proof does not invent a participant",
        ),
        Err(error) => map_account_link_write_error(&error),
    }
}

fn execute_unlink(
    runtime: &mut AccountLinkHttpRuntime,
    transaction: &mut Transaction<'_>,
    unlink: &AccountLinkUnlinkRequest,
) -> AccountLinkHttpResponse {
    let fingerprint = unlink.fingerprint();
    if let Some(existing) = runtime.idempotency.get(&unlink.idempotency_key) {
        if existing.fingerprint == fingerprint {
            return AccountLinkHttpResponse::json(existing.status, existing.body.clone());
        }
        return AccountLinkHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key was reused with a different account-link unlink body",
        );
    }
    let Ok(authenticated) = AuthenticatedAccountControl::new(
        &unlink.tenant_ref,
        &unlink.identity_issuer,
        &unlink.identity_subject_ref,
        &unlink.authenticated_proof_ref,
        unlink.authenticated_valid_until_unix_ms,
    ) else {
        return AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-authenticated-proof",
            "Invalid Authenticated Proof",
            "authenticated account-control fields must be opaque and currently valid",
        );
    };
    let mut participant = match recover_participant_for_authenticated_account(
        transaction,
        &authenticated,
        runtime.now_unix_ms,
    ) {
        Ok(Some(record)) => record,
        Ok(None) => {
            return AccountLinkHttpResponse::problem(
                404,
                "urn:psychometrics-commons:problem:account-link-not-found",
                "Account Link Not Found",
                "unlink recovers the current binding from the authenticated proof and does not accept a participant_ref grant",
            );
        }
        Err(error) => return map_account_link_write_error(&error),
    };
    match persist_authorized_account_unlink(
        transaction,
        &mut participant,
        &authenticated,
        &unlink.link_end_event_ref,
        runtime.now_unix_ms,
    ) {
        Ok(_) => {
            let body = account_link_body(&participant, "ended");
            runtime.idempotency.insert(
                unlink.idempotency_key.clone(),
                IdempotentPersist {
                    fingerprint,
                    status: 200,
                    body: body.clone(),
                },
            );
            AccountLinkHttpResponse::json(200, body)
        }
        Err(error) => map_account_link_write_error(&error),
    }
}

fn load_or_mint_participant(
    transaction: &mut Transaction<'_>,
    persist: &AccountLinkPersistRequest,
    now_unix_ms: u64,
) -> Result<ParticipantRecord, AccountLinkHttpResponse> {
    match load_participant_identity_history(
        transaction,
        &persist.participant_ref,
        &persist.tenant_ref,
    ) {
        Ok(Some(existing)) => Ok(existing),
        Ok(None) => ParticipantRecord::new_anonymous(
            &persist.participant_ref,
            &persist.tenant_ref,
            now_unix_ms,
        )
        .map_err(|_| {
            AccountLinkHttpResponse::problem(
                400,
                "urn:psychometrics-commons:problem:invalid-participant",
                "Invalid Participant",
                "account-link persist requires an opaque participant and tenant",
            )
        }),
        Err(error) => Err(map_account_link_write_error(&error.into())),
    }
}

fn map_account_link_write_error(error: &AccountLinkWriteError) -> AccountLinkHttpResponse {
    match error {
        AccountLinkWriteError::Authorization(authorization) => {
            map_account_link_authorization_error(*authorization)
        }
        AccountLinkWriteError::Persistence(persistence) => {
            map_identity_link_persistence_error(persistence)
        }
        AccountLinkWriteError::CurrentProjectionDrift => AccountLinkHttpResponse::problem(
            503,
            "urn:psychometrics-commons:problem:current-projection-drift",
            "Current Projection Drift",
            "operators must run restore reconcile before accepting new account-link writes",
        ),
        AccountLinkWriteError::NoCurrentBinding => AccountLinkHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:no-current-binding",
            "No Current Binding",
            "this authenticated account is not the participant's current identity link",
        ),
    }
}

fn map_account_link_authorization_error(
    error: AccountLinkAuthorizationError,
) -> AccountLinkHttpResponse {
    match error {
        AccountLinkAuthorizationError::InvalidTimestamp => AccountLinkHttpResponse::problem(
            500,
            "urn:psychometrics-commons:problem:server-clock",
            "Server Clock Error",
            "account-link persist, recover, and unlink require a server clock greater than zero",
        ),
        AccountLinkAuthorizationError::AnonymousSessionExpired
        | AccountLinkAuthorizationError::AuthenticatedProofExpired => {
            AccountLinkHttpResponse::problem(
                401,
                "urn:psychometrics-commons:problem:proof-expired",
                "Proof Expired",
                "both current proofs must still be valid at the server account-link time",
            )
        }
        AccountLinkAuthorizationError::AnonymousBindingMismatch
        | AccountLinkAuthorizationError::AuthenticatedBindingMismatch
        | AccountLinkAuthorizationError::CrossTenantDenied => AccountLinkHttpResponse::problem(
            403,
            "urn:psychometrics-commons:problem:account-link-forbidden",
            "Account Link Forbidden",
            "anonymous and authenticated proofs must belong to the same tenant and current binding",
        ),
        AccountLinkAuthorizationError::InvalidReference
        | AccountLinkAuthorizationError::InvalidValidityBoundary => {
            AccountLinkHttpResponse::problem(
                400,
                "urn:psychometrics-commons:problem:invalid-proof",
                "Invalid Proof",
                "account-link proofs must use opaque references and a positive validity boundary",
            )
        }
        AccountLinkAuthorizationError::Participant(
            AccountLinkError::AlreadyLinked
            | AccountLinkError::ConflictingReplay
            | AccountLinkError::ConflictingLinkEndReplay
            | AccountLinkError::ProofReferenceReuse,
        ) => AccountLinkHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:account-link-conflict",
            "Account Link Conflict",
            "this participant already has conflicting account-link evidence",
        ),
        AccountLinkAuthorizationError::Participant(
            AccountLinkError::InvalidReference
            | AccountLinkError::InvalidTimestamp
            | AccountLinkError::NonMonotonicTimestamp
            | AccountLinkError::NonMonotonicLifecycleTimestamp
            | AccountLinkError::NotLinked,
        ) => AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-account-link",
            "Invalid Account Link",
            "account-link persist rejected the participant lifecycle evidence",
        ),
    }
}

fn map_identity_link_persistence_error(
    error: &IdentityLinkPersistenceError,
) -> AccountLinkHttpResponse {
    match error {
        IdentityLinkPersistenceError::SubjectAlreadyBound
        | IdentityLinkPersistenceError::ConflictingReplay => AccountLinkHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:subject-already-bound",
            "Subject Already Bound",
            "this issuer-scoped subject already has a current participant identity link",
        ),
        IdentityLinkPersistenceError::InvalidReference
        | IdentityLinkPersistenceError::InvalidTimestamp => AccountLinkHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-account-link",
            "Invalid Account Link",
            "account-link persist rejected an opaque reference or timestamp",
        ),
        IdentityLinkPersistenceError::UnsupportedIsolationLevel
        | IdentityLinkPersistenceError::CorruptHistory
        | IdentityLinkPersistenceError::Database(_) => AccountLinkHttpResponse::problem(
            500,
            "urn:psychometrics-commons:problem:account-link-store",
            "Account Link Store Error",
            "account-link persist, recover, or unlink could not use the product store",
        ),
    }
}

fn account_link_body(participant: &ParticipantRecord, disposition: &str) -> String {
    format!(
        "{{\"participant_ref\":{},\"tenant_ref\":{},\"identity_issuer\":{},\"identity_subject_ref\":{},\"link_event_ref\":{},\"disposition\":{}}}",
        json_string(participant.participant_ref()),
        json_string(participant.tenant_ref()),
        json_string(participant.linked_issuer_ref().unwrap_or("")),
        json_string(participant.linked_subject_ref().unwrap_or("")),
        json_string(participant.link_event_ref().unwrap_or("")),
        json_string(disposition)
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum JsonAtom {
    String(String),
    Number(u64),
}

fn parse_persist_body(body: &str, idempotency_key: &str) -> Option<AccountLinkPersistRequest> {
    let fields = parse_object(body)?;
    if fields.len() != 10 {
        return None;
    }
    Some(AccountLinkPersistRequest {
        idempotency_key: idempotency_key.to_owned(),
        participant_ref: required_string(&fields, "participant_ref")?,
        tenant_ref: required_string(&fields, "tenant_ref")?,
        anonymous_session_ref: required_string(&fields, "anonymous_session_ref")?,
        anonymous_proof_ref: required_string(&fields, "anonymous_proof_ref")?,
        anonymous_valid_until_unix_ms: required_number(&fields, "anonymous_valid_until_unix_ms")?,
        identity_issuer: required_string(&fields, "identity_issuer")?,
        identity_subject_ref: required_string(&fields, "identity_subject_ref")?,
        authenticated_proof_ref: required_string(&fields, "authenticated_proof_ref")?,
        authenticated_valid_until_unix_ms: required_number(
            &fields,
            "authenticated_valid_until_unix_ms",
        )?,
        link_event_ref: required_string(&fields, "link_event_ref")?,
    })
}

fn parse_recover_body(body: &str) -> Option<AccountLinkRecoverRequest> {
    let fields = parse_object(body)?;
    if fields.len() != 5 {
        return None;
    }
    Some(AccountLinkRecoverRequest {
        tenant_ref: required_string(&fields, "tenant_ref")?,
        identity_issuer: required_string(&fields, "identity_issuer")?,
        identity_subject_ref: required_string(&fields, "identity_subject_ref")?,
        authenticated_proof_ref: required_string(&fields, "authenticated_proof_ref")?,
        authenticated_valid_until_unix_ms: required_number(
            &fields,
            "authenticated_valid_until_unix_ms",
        )?,
    })
}

fn parse_unlink_body(body: &str, idempotency_key: &str) -> Option<AccountLinkUnlinkRequest> {
    let fields = parse_object(body)?;
    if fields.len() != 6 || fields.contains_key("participant_ref") {
        return None;
    }
    Some(AccountLinkUnlinkRequest {
        idempotency_key: idempotency_key.to_owned(),
        tenant_ref: required_string(&fields, "tenant_ref")?,
        identity_issuer: required_string(&fields, "identity_issuer")?,
        identity_subject_ref: required_string(&fields, "identity_subject_ref")?,
        authenticated_proof_ref: required_string(&fields, "authenticated_proof_ref")?,
        authenticated_valid_until_unix_ms: required_number(
            &fields,
            "authenticated_valid_until_unix_ms",
        )?,
        link_end_event_ref: required_string(&fields, "link_end_event_ref")?,
    })
}

fn required_string(fields: &HashMap<String, JsonAtom>, name: &str) -> Option<String> {
    match fields.get(name)? {
        JsonAtom::String(value) => Some(value.clone()),
        JsonAtom::Number(_) => None,
    }
}

fn required_number(fields: &HashMap<String, JsonAtom>, name: &str) -> Option<u64> {
    match fields.get(name)? {
        JsonAtom::Number(value) => Some(*value),
        JsonAtom::String(_) => None,
    }
}

fn parse_object(input: &str) -> Option<HashMap<String, JsonAtom>> {
    let rest = input.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    if rest.is_empty() {
        return Some(HashMap::new());
    }
    let mut fields = HashMap::new();
    let mut remaining = rest;
    loop {
        let (key, after_key) = parse_json_string(remaining.trim_start())?;
        let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
        let (value, after_value) = parse_json_atom(after_colon)?;
        if fields.insert(key, value).is_some() {
            return None;
        }
        let after_value = after_value.trim_start();
        if after_value.is_empty() {
            return Some(fields);
        }
        remaining = after_value.strip_prefix(',')?.trim_start();
        if remaining.is_empty() {
            return None;
        }
    }
}

fn parse_json_atom(input: &str) -> Option<(JsonAtom, &str)> {
    if let Some((value, rest)) = parse_json_string(input) {
        return Some((JsonAtom::String(value), rest));
    }
    parse_json_u64(input).map(|(value, rest)| (JsonAtom::Number(value), rest))
}

fn parse_json_u64(input: &str) -> Option<(u64, &str)> {
    let digits = input
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit())
        .last()
        .map(|(index, character)| index + character.len_utf8())?;
    if digits == 0 {
        return None;
    }
    let value = input[..digits].parse().ok()?;
    Some((value, &input[digits..]))
}

fn parse_json_string(input: &str) -> Option<(String, &str)> {
    let rest = input.strip_prefix('"')?;
    let mut decoded = String::new();
    let mut chars = rest.char_indices();
    while let Some((index, character)) = chars.next() {
        match character {
            '"' => return Some((decoded, &rest[index + 1..])),
            '\\' => match chars.next()?.1 {
                '"' => decoded.push('"'),
                '\\' => decoded.push('\\'),
                'n' => decoded.push('\n'),
                'r' => decoded.push('\r'),
                't' => decoded.push('\t'),
                _ => return None,
            },
            character if character.is_control() => return None,
            character => decoded.push(character),
        }
    }
    None
}

fn valid_idempotency_key(value: &str) -> Option<&str> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.chars().any(char::is_whitespace) {
        return None;
    }
    let numeric_like = normalized.chars().any(char::is_numeric)
        && normalized
            .chars()
            .all(|character| character.is_numeric() || matches!(character, '+' | '-' | '.' | ','));
    if numeric_like {
        None
    } else {
        Some(normalized)
    }
}

fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().skip(1).find_map(|line| {
        if line.is_empty() {
            return None;
        }
        let (header_name, value) = line.split_once(':')?;
        header_name.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn request_body(request: &str) -> Option<&str> {
    let (headers, body) = request.split_once("\r\n\r\n")?;
    let content_length = header_value(headers, "content-length")?
        .parse::<usize>()
        .ok()?;
    if body.len() < content_length {
        return None;
    }
    Some(&body[..content_length])
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let read_result = stream.read(&mut chunk);
        match apply_request_read(&mut buffer, &chunk, read_result)? {
            RequestReadProgress::Continue => {}
            RequestReadProgress::Complete => break,
        }
    }
    if buffer.len() > ACCOUNT_LINK_HTTP_MAX_REQUEST_BYTES
        || !buffer.windows(4).any(|window| window == b"\r\n\r\n")
    {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[derive(Debug)]
enum RequestReadProgress {
    Continue,
    Complete,
}

fn request_bytes_are_complete(buffer: &[u8]) -> bool {
    if buffer.len() > ACCOUNT_LINK_HTTP_MAX_REQUEST_BYTES {
        return true;
    }
    let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let header_bytes = header_end + 4;
    let Ok(headers) = std::str::from_utf8(&buffer[..header_bytes]) else {
        return true;
    };
    let Some(content_length) =
        header_value(headers, "content-length").and_then(|value| value.parse::<usize>().ok())
    else {
        return true;
    };
    buffer.len().saturating_sub(header_bytes) >= content_length
}

fn apply_request_read(
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    read_result: io::Result<usize>,
) -> io::Result<RequestReadProgress> {
    match read_result {
        Ok(0) => Ok(RequestReadProgress::Complete),
        Ok(read) => {
            buffer.extend_from_slice(&chunk[..read]);
            if request_bytes_are_complete(buffer) {
                Ok(RequestReadProgress::Complete)
            } else {
                Ok(RequestReadProgress::Continue)
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(RequestReadProgress::Complete)
        }
        Err(error) => Err(error),
    }
}

fn write_http_response(
    stream: &mut TcpStream,
    response: &AccountLinkHttpResponse,
) -> io::Result<()> {
    let body = response.body().as_bytes();
    let allow = if response.status() == 405 {
        "Allow: POST\r\n"
    } else {
        ""
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{allow}Connection: close\r\n\r\n",
        response.status(),
        reason_phrase(response.status()),
        response.content_type(),
        body.len()
    );
    io::Write::write_all(stream, header.as_bytes())?;
    io::Write::write_all(stream, body)
}

const fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if !version.starts_with("HTTP/") || parts.next().is_some() {
        return None;
    }
    Some((method, target))
}

fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

fn json_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::{
        apply_request_read, bind_account_link_http, classify_account_link_http_request,
        json_string, map_account_link_write_error, parse_json_string, parse_object,
        parse_persist_body, parse_unlink_body, reason_phrase, valid_idempotency_key,
        AccountLinkHttpClassification, RequestReadProgress, ACCOUNT_LINK_HTTP_MAX_REQUEST_BYTES,
    };
    use crate::account_link::AccountLinkAuthorizationError;
    use crate::account_link_write::AccountLinkWriteError;
    use crate::participant::AccountLinkError;
    use crate::postgres_participant_identity_link::IdentityLinkPersistenceError;
    use std::io::{self, ErrorKind};
    use std::net::SocketAddr;

    #[test]
    fn remaining_phrases_escapes_and_parse_edges_are_stable() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(401), "Unauthorized");
        assert_eq!(reason_phrase(403), "Forbidden");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(503), "Service Unavailable");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(valid_idempotency_key("  "), None);
        assert_eq!(valid_idempotency_key("has space"), None);
        assert_eq!(valid_idempotency_key("12345"), None);
        assert_eq!(valid_idempotency_key("idem_ok"), Some("idem_ok"));
        assert!(parse_object("{").is_none());
        assert!(parse_object("{\"a\":\"1\",\"a\":\"2\"}").is_none());
        assert!(parse_object("{\"a\":\"1\",}").is_none());
        assert!(parse_persist_body("{\"participant_ref\":\"p\"}", "idem_ok").is_none());
        assert!(parse_unlink_body(
            "{\"participant_ref\":\"stolen\",\"tenant_ref\":\"t\",\"identity_issuer\":\"i\",\"identity_subject_ref\":\"s\",\"authenticated_proof_ref\":\"p\",\"authenticated_valid_until_unix_ms\":1,\"link_end_event_ref\":\"e\"}",
            "idem_ok"
        )
        .is_none());
        assert!(parse_json_string("\"unterminated").is_none());
        assert!(parse_json_string("\"bad\\x\"").is_none());
        assert!(parse_json_string("\"\u{0001}\"").is_none());
        let (decoded, rest) = parse_json_string("\"a\\\"b\\\\c\\n\\r\\t\"tail").unwrap();
        assert_eq!(decoded, "a\"b\\c\n\r\t");
        assert_eq!(rest, "tail");
        assert!(parse_object("{\"n\":\"s\"}").unwrap().contains_key("n"));
    }

    #[test]
    fn request_read_progress_and_transport_failures_are_classified() {
        let mut buffer = Vec::new();
        assert!(matches!(
            apply_request_read(&mut buffer, b"", Ok(0)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(&mut Vec::new(), b"GET", Ok(3)).unwrap(),
            RequestReadProgress::Continue
        ));
        let mut oversized = vec![b'x'; ACCOUNT_LINK_HTTP_MAX_REQUEST_BYTES];
        assert!(matches!(
            apply_request_read(&mut oversized, b"y", Ok(1)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(
                &mut Vec::new(),
                b"",
                Err(io::Error::new(ErrorKind::TimedOut, "timeout"))
            )
            .unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(
                &mut Vec::new(),
                b"",
                Err(io::Error::new(ErrorKind::WouldBlock, "block"))
            )
            .unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(apply_request_read(&mut Vec::new(), b"", Err(io::Error::other("boom"))).is_err());
        let mut headers_only = Vec::new();
        let header_chunk = b"POST /v1/account-links/recover HTTP/1.1\r\nContent-Length: 2\r\n\r\n";
        assert!(matches!(
            apply_request_read(&mut headers_only, header_chunk, Ok(header_chunk.len())).unwrap(),
            RequestReadProgress::Continue
        ));
        assert!(matches!(
            apply_request_read(&mut headers_only, b"{}", Ok(2)).unwrap(),
            RequestReadProgress::Complete
        ));
        let get_chunk = b"GET /live HTTP/1.1\r\n\r\n";
        assert!(matches!(
            apply_request_read(&mut Vec::new(), get_chunk, Ok(get_chunk.len())).unwrap(),
            RequestReadProgress::Complete
        ));
    }

    #[test]
    fn write_errors_map_to_safe_problem_statuses() {
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::InvalidTimestamp
            ))
            .status(),
            500
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::AuthenticatedProofExpired
            ))
            .status(),
            401
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::AnonymousSessionExpired
            ))
            .status(),
            401
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::CrossTenantDenied
            ))
            .status(),
            403
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::AnonymousBindingMismatch
            ))
            .status(),
            403
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::AuthenticatedBindingMismatch
            ))
            .status(),
            403
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::InvalidReference
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::InvalidValidityBoundary
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::CurrentProjectionDrift).status(),
            503
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::NoCurrentBinding).status(),
            409
        );
    }

    #[test]
    fn participant_lifecycle_write_errors_map_to_safe_problem_statuses() {
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::AlreadyLinked)
            ))
            .status(),
            409
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::ProofReferenceReuse)
            ))
            .status(),
            409
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::ConflictingReplay)
            ))
            .status(),
            409
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(
                    AccountLinkError::ConflictingLinkEndReplay
                )
            ))
            .status(),
            409
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::NotLinked)
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::InvalidReference)
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::InvalidTimestamp)
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(AccountLinkError::NonMonotonicTimestamp)
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Authorization(
                AccountLinkAuthorizationError::Participant(
                    AccountLinkError::NonMonotonicLifecycleTimestamp
                )
            ))
            .status(),
            400
        );
    }

    #[test]
    fn persistence_write_errors_map_to_safe_problem_statuses() {
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Persistence(
                IdentityLinkPersistenceError::SubjectAlreadyBound
            ))
            .status(),
            409
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Persistence(
                IdentityLinkPersistenceError::ConflictingReplay
            ))
            .status(),
            409
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Persistence(
                IdentityLinkPersistenceError::InvalidReference
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Persistence(
                IdentityLinkPersistenceError::InvalidTimestamp
            ))
            .status(),
            400
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Persistence(
                IdentityLinkPersistenceError::UnsupportedIsolationLevel
            ))
            .status(),
            500
        );
        assert_eq!(
            map_account_link_write_error(&AccountLinkWriteError::Persistence(
                IdentityLinkPersistenceError::CorruptHistory
            ))
            .status(),
            500
        );
    }

    #[test]
    fn classify_covers_recover_unlink_and_nested_account_link_paths() {
        assert!(matches!(
            classify_account_link_http_request("NOT-A-REQUEST"),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        assert!(matches!(
            classify_account_link_http_request("PUT /v1/account-links/recover HTTP/1.1\r\n\r\n"),
            AccountLinkHttpClassification::Ready(response) if response.status() == 405
        ));
        assert!(matches!(
            classify_account_link_http_request("PUT /v1/account-links/unlink HTTP/1.1\r\n\r\n"),
            AccountLinkHttpClassification::Ready(response) if response.status() == 405
        ));
        assert!(matches!(
            classify_account_link_http_request("POST /v1/account-links/other HTTP/1.1\r\n\r\n"),
            AccountLinkHttpClassification::Ready(response) if response.status() == 405
        ));
        assert!(matches!(
            classify_account_link_http_request(
                "POST /v1/account-links/recover HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"
            ),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        assert!(matches!(
            classify_account_link_http_request(
                "POST /v1/account-links/unlink HTTP/1.1\r\nIdempotency-Key: idem_ok\r\n\r\n{}"
            ),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        assert!(matches!(
            classify_account_link_http_request(
                "POST /v1/account-links HTTP/1.1\r\nIdempotency-Key: idem_ok\r\n\r\n{}"
            ),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        assert!(matches!(
            classify_account_link_http_request(
                "POST /v1/account-links/recover HTTP/1.1\r\nContent-Length: 16\r\n\r\n{\"tenant_ref\":1}"
            ),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        assert!(matches!(
            classify_account_link_http_request(
                "POST /v1/account-links HTTP/1.1\r\nIdempotency-Key: idem_ok\r\nContent-Length: 40\r\n\r\n{\"anonymous_valid_until_unix_ms\":\"11000\"}"
            ),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        assert!(matches!(
            classify_account_link_http_request(
                "POST /v1/account-links/recover HTTP/1.1\r\nContent-Length: 20\r\n\r\n{\"tenant_ref\":\"t\"}"
            ),
            AccountLinkHttpClassification::Ready(response) if response.status() == 400
        ));
        let listener =
            bind_account_link_http("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
        assert_ne!(listener.local_addr().unwrap().port(), 0);
    }
}
