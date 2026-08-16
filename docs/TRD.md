# Technical Requirements Document — Psychometrics Commons

- Status: Implementation baseline
- Version: 0.1
- Date: 2026-08-09

## 1. Architectural objective

Psychometrics Commons is a headless hosted product that composes reusable CWL services without reversing their dependency direction or sharing application databases.

Canonical dependency direction:

```text
clients
  -> psychometrics-commons
      -> fast-mlsirm
      -> Keyverse
      -> Gyeot / TEPP
      -> semantic-data-portal
      -> contextual-orchestrator / pg-llm-batch
      -> EgressWeave
      -> optional Inkspan / RankWeave / Clearfolio integrations
```

Forbidden dependency examples:

```text
fast-mlsirm -> psychometrics-commons
TEPP -> psychometrics-commons database
semantic-data-portal -> psychometrics-commons operational database
browser -> internal fast-mlsirm kernel ABI
```

## 2. System of record

| Domain | System of record | Integration surface |
|---|---|---|
| authentication, federation, passkeys, account linking | Keyverse | OIDC/OAuth and versioned claims |
| instrument publication, participant/session, response, consent, result | psychometrics-commons | public/admin APIs and domain events |
| AssessmentSpec, RubricSpecification, scoring contracts and psychometric numerics | fast-mlsirm | versioned package/service contracts |
| mobile EMA/ESM local collection | Gyeot | sync contract and observation events |
| temporal/event/multilevel/multiple-membership analysis | TEPP | analysis-job and immutable-artifact contracts |
| research release catalog, lineage, license, discovery | semantic-data-portal | release manifest registration |
| bounded real-time AI | contextual-orchestrator | task/result contracts |
| bulk asynchronous model execution | pg-llm-batch | batch-job contracts |
| outbound provider security | EgressWeave | egress policy/enforcement interface |

No service may possess another service's normal application-database credentials.

## 3. Runtime modules

The product runtime is logically decomposed into the following modules even if initially deployed as one service:

- `instrument_publication`
- `assessment_session`
- `item_delivery`
- `response_event`
- `consent_record`
- `scoring_dispatch`
- `result_snapshot`
- `data_rights`
- `research_contribution`
- `integration_outbox`
- `tenant_authorization`

Module boundaries must be visible in code and schema ownership. They may share one product database initially but may not bypass each other's invariants through ad-hoc table mutation.

## 4. Public identifier contract

Public resource identifiers must be opaque, non-numeric, globally collision-resistant references. Sequential database primary keys must not cross the API boundary.

Every externally persisted resource carries:

- a public resource reference;
- creation time assigned by the server;
- schema or resource version when semantics can evolve;
- tenant context where applicable;
- immutable or supersession metadata where applicable.

Raw database IDs, Keyverse subjects, linkage keys, and provider credentials must not appear in URLs intended for public sharing.

## 5. Session state machine

Canonical lifecycle:

```text
created -> active <-> paused -> completed -> scoring -> scored -> released
```

Terminal alternatives:

```text
expired | cancelled | invalidated
```

Requirements:

- clients submit commands, never arbitrary target states;
- duplicate equivalent commands are idempotent;
- undocumented transitions fail closed;
- only `active` accepts new normal response events;
- completion atomically freezes a `response_snapshot_ref`;
- scoring dispatch occurs only after the completion transaction and snapshot durability;
- released results are not mutated in place;
- corrections use audited superseding result snapshots.

## 6. Response event contract

A response event must include or resolve:

```text
response_event_ref
session_ref
item_version_ref
client_event_ref
server_sequence
observed_at optional
received_at
response_schema_version
response_payload or encrypted_payload_ref
locale
presentation_context_ref optional
```

The server assigns authoritative sequence/order. Client timestamps are evidence, not the sole ordering authority.

Idempotency requirements:

- duplicate `client_event_ref` with identical canonical content returns the original accepted outcome;
- reuse of an idempotency key with different canonical content is rejected;
- replay after reconnect cannot create a second logical response event;
- completion waits for all server-acknowledged required events or applies the published missingness policy.

## 7. Instrument publication contract

Canonical state:

```text
draft -> review -> published -> suspended -> retired
```

A suspended release may be reactivated only when the same immutable bytes and policy are resumed; content changes require a new version.

Published instrument versions must pin:

- construct/instrument reference;
- item-version set and order/selection policy;
- locale;
- AssessmentSpec reference;
- scoring-version reference;
- calibration reference;
- norm-version reference where applicable;
- narrative-rule version;
- consent requirements;
- intended-use and limitations metadata.

Retirement blocks new sessions but does not invalidate historical provenance.

## 8. Scoring dispatch contract

Psychometrics Commons does not calculate psychometric numerics locally when the calculation belongs to fast-mlsirm.

A scoring request must pin:

```text
scoring_request_ref
response_snapshot_ref
assessment_spec_ref
instrument_version_ref
scoring_version_ref
calibration_reference
norm_version_ref optional
requested_output_schema_version
```

The scoring response must provide:

```text
scoring_result_ref
status
score observations
uncertainty/diagnostic fields supported by the contract
engine/package artifact version or digest
warnings and typed scientific failures
```

Required status separation:

- scored
- abstained
- failed
- excluded

A missing value must not be represented as score zero.

Scientific failures such as non-identification, non-finite results, insufficient linking anchors, unsupported contract major versions, or scoreability failures must fail closed rather than silently switching model semantics.

## 9. Result snapshot contract

Each released result snapshot must identify:

```text
result_snapshot_ref
participant_ref
response_snapshot_ref
instrument_version_ref
assessment_spec_ref
scoring_version_ref
calibration_reference
norm_version_ref optional
narrative_version_ref
consent_snapshot_refs
engine_artifact_digest
created_at
supersedes_ref optional
```

Numeric score content and narrative content are independently versioned. Updating narrative rules or norms never mutates the historical result. Rescoring creates a superseding snapshot.

## 10. Identity and authorization

Keyverse token validation must verify issuer, audience, signature, expiry, and other protocol-required anti-replay fields.

Psychometrics Commons owns resource authorization decisions for:

- participant-owned result access;
- instrument administration;
- research roles;
- release approval;
- export/deletion requests;
- tenant-scoped resources.

A Keyverse administrative role does not implicitly grant research-release approval.

Anonymous sessions use product-issued short-lived credentials and pseudonymous participant references. Linking to a Keyverse subject requires proof of control of both identities.

## 11. Tenant isolation

Every tenant-scoped state-changing operation must derive tenant context from authenticated authorization rather than an untrusted request body default.

Required negative tests include:

- cross-tenant read by valid authenticated subject;
- cross-tenant mutation using a guessed public reference;
- cross-tenant idempotency-key collision attempts;
- result-sharing token reuse across tenant/resource audience;
- admin-role confusion between identity and product domains.

No implicit `default` tenant is permitted for state-changing production APIs.

## 12. Consent model

Consent records are immutable snapshots. Revocation creates a later consent event.

At minimum, separate purposes are represented for:

- core service processing;
- optional account persistence;
- optional longitudinal processing;
- optional research contribution;
- optional communications.

A research contribution record must point to the exact consent-form version and allowed research scope.

Optional research consent is not a prerequisite for accessing the personal assessment result.

## 13. Data-rights workflows

Export and deletion are durable resources rather than synchronous best-effort endpoints.

State model:

```text
requested -> identity_verified -> processing -> completed
```

Terminal alternatives:

```text
rejected | partially_retained_with_basis | failed
```

The workflow records requested scope, verification evidence reference, legal/contractual retention exceptions, dependent-system propagation, completion evidence, and audit timestamps.

## 14. Research pseudonymization boundary

Operational participant identity and research participant identity are separate namespaces.

The restricted linkage service/table is the only place that may connect:

```text
assessment_participant_ref <-> research_participant_ref
```

Release snapshots are generated only from research-domain identifiers and approved variables. Release validation fails if an operational identity field or Keyverse subject is present.

## 15. Research release manifest

Release registration with semantic-data-portal is idempotent by release reference and manifest digest.

The manifest references immutable artifacts and includes:

- dataset digest and format;
- codebook and variable dictionary digests;
- data-card digest;
- license and consent-scope metadata;
- instrument/item/scoring/calibration/norm provenance;
- privacy-review decision;
- citation metadata;
- release access class;
- supersession relations.

Digest mismatch for an existing release reference is fatal.

## 16. Longitudinal ingestion

For EMA/ESM observations, preserve distinct event-time fields where present:

```text
observed_at
recorded_at
received_at
available_at
valid_from
valid_to
original_timezone
```

Multiple-membership context is represented explicitly rather than collapsed to one group.

Sync uses stable client observation references and canonical content digests. Conflicting edits create superseding or adjudication records; blind last-write-wins is forbidden for scientifically meaningful observations.

## 17. AI task boundary

Every AI task declares:

- task type and purpose;
- exact model/provider routing policy;
- allowed input data classes;
- residency and retention policy;
- schema version for output;
- maximum payload and output sizes;
- timeout and retry policy;
- provenance fields required for audit.

Provider output is untrusted. Parsing rejects duplicate JSON keys, unknown required semantics, non-finite numbers, oversized content, invalid references, and provenance mismatch.

A failure in AI narration must fall back to deterministic approved content rather than blocking scoring or result retrieval.

## 18. API requirements

Initial public API families:

```text
GET    /v1/instruments
GET    /v1/instruments/{instrument_ref}
POST   /v1/sessions
GET    /v1/sessions/{session_ref}
POST   /v1/sessions/{session_ref}/responses
POST   /v1/sessions/{session_ref}/commands
GET    /v1/results/{result_ref}
POST   /v1/results/{result_ref}/exports
POST   /v1/consents
POST   /v1/research-contributions
POST   /v1/research-contributions/{contribution_ref}/withdrawals
POST   /v1/data-rights/exports
POST   /v1/data-rights/deletions
POST   /v1/account-links
POST   /v1/account-links/recover
POST   /v1/account-links/unlink
```

All state-changing public requests require an idempotency key or a resource-specific equivalent.

API errors use stable machine codes plus safe human text. Raw provider, SQL, credential, or sensitive response data must not be included in client error bodies.

## 19. Event requirements

Initial domain-event families:

```text
assessment.session.created
assessment.response.recorded
assessment.session.completed
assessment.scoring.requested
assessment.scoring.completed
assessment.result.released
consent.research.granted
consent.research.withdrawn
research.snapshot.requested
research.release.approved
data_rights.export.requested
data_rights.deletion.requested
```

Every event includes:

```text
event_ref
event_type
schema_version
source
subject_ref
occurred_at
correlation_ref
causation_ref optional
payload_digest
payload
```

Consumers must deduplicate before applying side effects.

## 20. Transactional integration

State change and durable outbound event creation occur in one local transaction through a transactional outbox.

Consumers use an inbox/deduplication record before side effects.

Requirements:

- at-least-once transport must not create duplicate domain effects;
- retry backoff is bounded and observable;
- poison messages are quarantined with typed cause;
- a dependent-service outage does not roll back an already-valid local participant action;
- reconciliation compares event/resource digests rather than assuming last-write-wins.

## 21. Database rules

Database objects use descriptive names of at least two words in `snake_case` by default.

Examples:

```text
instrument_definition
instrument_version
assessment_participant
assessment_session
response_event
response_snapshot
scoring_job
result_snapshot
consent_form
consent_snapshot
research_contribution
research_participant
dataset_snapshot
research_release
data_rights_request
integration_outbox
integration_inbox
```

Published immutable objects must not be updated in place except for explicitly non-semantic operational metadata whose mutation is separately audited.

## 22. Observability

Every state-changing request and integration operation carries a correlation reference.

Minimum telemetry:

- request latency and result class;
- command transition accepted/rejected;
- response-event deduplication/conflict counts;
- scoring job state and typed failure class;
- outbox/inbox age and retry count;
- optional dependency availability;
- data-rights age and completion;
- release-registration reconciliation state.

Logs use resource references and digests. Raw assessment responses, credentials, and restricted linkage values must not appear in routine logs.

## 23. Failure/degraded-mode matrix

| Failure | Required behavior |
|---|---|
| Keyverse unavailable | anonymous path and already-valid short-lived sessions continue where safe; new authenticated flows degrade explicitly |
| fast-mlsirm/scoring unavailable | session completion and immutable response snapshot remain durable; scoring waits/retries without inventing results |
| contextual-orchestrator unavailable | numeric result remains available with deterministic narrative fallback |
| semantic-data-portal unavailable | personal results unaffected; approved release registration remains queued |
| TEPP unavailable | longitudinal observations remain durable; analysis artifact generation waits |
| Egress policy denies provider | AI capability fails closed; no direct bypass call |
| database transaction fails | command has no partial state transition or orphan event |
| duplicate event delivered | inbox deduplication prevents duplicate side effects |

## 24. Version compatibility

Every external contract carries an explicit schema version.

Rules:

- additive optional fields may be introduced within a supported major version only when prior semantics do not change;
- unknown required semantics fail closed;
- major-version removal requires a documented compatibility window and migration path;
- historical result/release readers must remain available for the supported retention period;
- mutable aliases such as `latest` are resolved before an operation and never stored as provenance.

## 25. Security requirements

- least-privilege service and workflow credentials;
- immutable action/source pins where practical;
- encrypted transport and storage according to deployment policy;
- explicit secret-manager integration rather than committed credentials;
- exact audience/issuer validation on identity tokens;
- tenant-scoped server authorization;
- no direct external-provider credentials in clients;
- EgressWeave or equivalent exact-authority controls for model/provider calls;
- audit trails for privileged data access, consent changes, account linking, release approvals, and data-rights completion;
- SBOM, dependency review, static analysis, secret scanning, and provenance gates on release candidates.

## 26. Privacy requirements

Privacy controls use separation and authorization rather than blanket masking that makes authorized workflows unusable.

Required boundaries:

- identity data;
- operational assessment data;
- restricted linkage data;
- research staging data;
- approved research release data.

AI tasks and exports use explicit purpose-specific projections. Unauthorized contexts receive no field rather than a misleading masked field when omission is the correct control.

## 27. Accessibility requirements

Reference clients target WCAG 2.2 AA.

Required acceptance areas:

- keyboard-only assessment completion;
- screen-reader labels and state changes;
- focus management and error association;
- timing accommodation where timing is part of an instrument;
- non-color-only result communication;
- text/table equivalent for charts;
- zoom/reflow and target-size behavior;
- assistive-technology end-to-end verification.

Accessibility presentation differences that can affect measurement are recorded in instrument/version evidence rather than silently treated as cosmetic.

## 28. Multilingual requirements

Assessment content resolves an exact BCP 47 locale-specific published instrument version.

No silent fallback is permitted for item content. UI chrome may use a separately disclosed fallback policy.

Cross-locale comparison or shared norms require linking/invariance evidence. A locale may be published for within-locale self-reflection with an explicit limitation even when cross-locale comparability has not been demonstrated.

## 29. CI and validation requirements

Owned production code targets exact 100% statement and branch coverage plus line/function/region coverage where tooling exposes it, without excluding real behavior.

Required test classes as implementation grows:

- unit and property tests;
- state-machine exhaustive transition tests;
- API/schema contract tests;
- idempotency/concurrency tests;
- transaction/outbox failure-injection tests;
- tenant-authorization negative tests;
- privacy boundary and identity-leak tests;
- migration/rollback tests;
- accessibility tests;
- upstream consumer-driven contract tests;
- packaging/reproducible-build tests;
- scientific acceptance fixtures delegated to fast-mlsirm evidence.

## 30. Deployment profiles

### Community/Research

Must run with the product runtime, operational database, a fast-mlsirm-compatible scoring path, and a standalone client. AI, TEPP, semantic-data-portal, g7 and other optional integrations can be absent.

### CWL Hosted

May enable all CWL bounded-context integrations, managed observability, research-release registration, and bounded AI.

### Enterprise/Self-hosted

Adds deployment-specific identity federation, data residency, encryption, retention, model-provider, networking, and audit policies while keeping the same domain contracts and historical result portability.

## 31. Release gates

A product release is blocked unless the exact integrated protected head demonstrates:

- CI and exact owned-code coverage;
- SAST/security/dependency/secret gates;
- reproducible packaging and SBOM/provenance;
- supported-version compatibility;
- migration and rollback evidence;
- required independent review;
- reference-client accessibility acceptance;
- failure/degraded-mode acceptance;
- operational runbook readiness.

An instrument release additionally requires rights, locale/translation, calibration/scoring/norm, invariance where claimed, narrative-rule, and intended-use evidence.
