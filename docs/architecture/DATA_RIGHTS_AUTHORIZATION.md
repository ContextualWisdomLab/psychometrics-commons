# Data-rights authorization boundary

- Maturity: **PARTIAL** — the domain composition exists, while hosted route/repository integration and external identity-proof execution remain outside this slice.
- Ownership: `ContextualWisdomLab/psychometrics-commons`
- Governing contracts: PRD §11; TRD §10, §11, and §13; `docs/architecture/SECURITY_AND_DATA.md`

## Purpose

A data export or deletion request is sensitive product state. `DataRightsRequest` is the authoritative product record for the request's tenant, participant owner, and opaque `request_ref`. Authorization must therefore bind to that stored record rather than to copies of those identifiers supplied in a URL, request body, or authenticated actor context.

`src/data_rights_authorization.rs::authorize_data_rights_request` is the composition boundary. It derives a participant-owned `ResourceScope` only from the stored `DataRightsRequest` and evaluates the existing `ManageOwnDataRights` permission. `src/lib.rs` exposes that module as product API surface; it does not transfer ownership to Keyverse or to a transport adapter.

## Trust boundary

```text
authenticated actor context
        |
        v
stored DataRightsRequest ------------------+
(tenant_ref, participant_ref, request_ref)  |
        |                                    |
        +---------- authoritative identity --+
                                             v
                              participant-owned ResourceScope
                                             |
                                             v
                                   ManageOwnDataRights
                                             |
                              allow only exact owner/tenant
```

The actor proves who is asking; the stored request proves which product resource is being asked about. The server never substitutes the actor's own tenant, participant, or path parameter for a stored request attribute. If any required binding is missing, malformed, cross-tenant, or owned by another participant, authorization denies access.

## Unchanged architecture

This change does **not** add a new permission, credential system, data-rights lifecycle state, database object, cross-service database access, or identity-provider responsibility. Keyverse remains the authentication/federation owner; Psychometrics Commons remains the owner of product authorization and the data-rights aggregate. Because ownership, dependency direction, state machine, and persistence schema are unchanged, no superseding ADR is required. The implementation narrows an existing TRD §11 authorization invariant at the adapter-facing boundary.

## Required evidence

- `tests/data_rights_authorization_binding.rs` proves exact stored-owner success and fail-closed cross-tenant, cross-participant, and missing-identity cases.
- `src/data_rights_authorization.rs` unit tests prove malformed stored identity cannot construct an authorization scope.
- `tests/data_rights_authorization_documentation.rs` prevents the trust-boundary mapping from disappearing silently.
- Hosted HTTP/repository adapters must call this stored-record composition before reading, exporting, deleting, or disclosing request state; transport-level negative tests remain required before GA.
