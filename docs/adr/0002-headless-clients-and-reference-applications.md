# ADR-0002: Headless clients and replaceable reference applications

- Status: Accepted
- Date: 2026-08-09
- Scope: web, mobile, embed, research CLI, g7 integration

## Context

The platform needs public assessment, researcher workbench, administrative, mobile longitudinal, and institutional embed experiences. Making one UI framework canonical would couple product behavior to a CMS or frontend release cycle and would prevent customers from using their own client.

## Decision

Psychometrics Commons is headless. The product's canonical behavior is exposed through versioned APIs and events. `g7` is an optional reference web client, not a runtime dependency or source of truth. Standalone web, Gyeot, LifeOS integrations, institutional clients, and embed widgets are peers that consume the same contracts.

Client-specific BFFs may exist, but they may aggregate and transform presentation data only. They cannot own instrument versions, scoring state, consent state, or result provenance.

## Contract details

- Public and administrative APIs are documented through OpenAPI.
- Event contracts are separately versioned and are not inferred from UI payloads.
- Every write accepts an idempotency key.
- Pagination is cursor-based; public IDs are opaque and non-numeric.
- Clients send locale, timezone, accessibility preferences, and supported contract versions explicitly.
- API evolution is additive within a major version. Removing or changing semantics requires a new major version and a documented compatibility window.

## Invariants

1. Removing g7 does not disable assessment, scoring, export, or research contribution.
2. No client receives database credentials or internal service tokens.
3. A client cannot submit a score, norm, or narrative version that the server has not published.
4. Browser code never holds long-lived service credentials.
5. Embed clients are origin-restricted and use short-lived, audience-bound session tokens.

## Failure and degraded modes

A client may cache static instrument metadata but not mutable session authorization. Network interruption preserves local unsent responses with deterministic replay and idempotency. If a client version becomes incompatible, the API returns a typed upgrade-required response rather than accepting an ambiguous payload.

## Security and privacy

Content Security Policy, origin allowlists, anti-CSRF protections for cookie sessions, and proof-of-possession or short-lived bearer tokens apply according to client type. Result-sharing links are revocable, expire by default, and never expose raw response data unless explicitly selected by the participant.

## Validation

- contract tests run against at least one standalone web client and one non-web client;
- end-to-end tests prove anonymous and account-linked flows without g7;
- embed tests verify origin isolation and token audience;
- accessibility acceptance is enforced independently of the client framework.

## Alternatives rejected

- **g7 as canonical product shell:** useful for speed but rejected as a mandatory dependency.
- **Separate backend per client:** rejected because it fragments domain rules and provenance.
- **Direct browser calls to every downstream CWL service:** rejected because it leaks topology and multiplies authorization boundaries.

## Reversal conditions

Revisit if a regulated deployment requires a single certified client. That client may become the only approved presentation surface for that deployment, but the core APIs remain headless.
