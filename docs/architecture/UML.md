# UML-Aligned Domain and Behavior Models

- Status: Normative architecture view
- Date: 2026-08-09
- Notation: Mermaid diagrams using OMG UML 2.5.1 structural/behavioral semantics where the notation permits
- Authority: accepted ADRs and PRD/TRD override a diagram if a contradiction is discovered

These diagrams describe the intended product semantics. They do **not** claim that every class or transport adapter shown is already implemented on protected main. Current implementation evidence is tracked separately in `docs/TRACEABILITY.md`.

## 1. Domain class model

```mermaid
classDiagram
    class InstrumentDefinition {
      +instrument_ref
      +construct_ref
    }
    class InstrumentVersion {
      +instrument_version_ref
      +locale
      +assessment_spec_ref
      +scoring_version_ref
      +calibration_reference
      +norm_version_ref?
      +narrative_version_ref
      +content_digest
      +publication_state
    }
    class ItemVersion {
      +item_version_ref
      +content_digest
      +response_schema_version
    }
    class AssessmentParticipant {
      +participant_ref
      +tenant_ref
      +keyverse_subject_ref?
    }
    class AssessmentSession {
      +session_ref
      +instrument_version_ref
      +participant_ref
      +state
      +created_at
    }
    class ResponseEvent {
      +response_event_ref
      +client_event_ref
      +item_version_ref
      +payload_digest
      +server_sequence
    }
    class ResponseSnapshot {
      +response_snapshot_ref
      +session_ref
      +event_count
      +last_sequence
      +content_digest
    }
    class ScoringJob {
      +scoring_job_ref
      +response_snapshot_ref
      +assessment_spec_ref
      +scoring_version_ref
      +calibration_reference
      +norm_version_ref?
      +state
    }
    class ResultSnapshot {
      +result_snapshot_ref
      +response_snapshot_ref
      +scoring_result_ref
      +narrative_version_ref
      +engine_artifact_digest
      +supersedes_ref?
    }
    class ConsentSnapshot {
      +consent_snapshot_ref
      +participant_ref
      +consent_form_version_ref
      +purpose
      +decision
      +effective_at
    }
    class ResearchContribution {
      +contribution_ref
      +participant_ref
      +research_participant_ref
      +scope_ref
      +state
    }
    class DataRightsRequest {
      +request_ref
      +tenant_ref
      +participant_ref
      +kind
      +scope_ref
      +state
    }
    class DatasetSnapshot {
      +dataset_snapshot_ref
      +manifest_digest
      +privacy_review_ref
      +state
    }
    class ResearchRelease {
      +research_release_ref
      +dataset_snapshot_ref
      +manifest_digest
      +access_class
      +supersedes_ref?
    }

    InstrumentDefinition "1" --> "1..*" InstrumentVersion : versions
    InstrumentVersion "1" --> "1..*" ItemVersion : publishes ordered set
    AssessmentParticipant "1" --> "0..*" AssessmentSession : owns
    InstrumentVersion "1" --> "0..*" AssessmentSession : administers
    AssessmentSession "1" --> "0..*" ResponseEvent : records
    AssessmentSession "1" --> "0..1" ResponseSnapshot : freezes
    ResponseSnapshot "1" --> "0..*" ScoringJob : scored by
    ScoringJob "1" --> "0..1" ResultSnapshot : produces
    AssessmentParticipant "1" --> "0..*" ConsentSnapshot : decisions
    AssessmentParticipant "1" --> "0..*" ResearchContribution : opts into
    AssessmentParticipant "1" --> "0..*" DataRightsRequest : requests
    ResearchContribution "0..*" --> "0..*" DatasetSnapshot : eligible input
    DatasetSnapshot "1" --> "0..*" ResearchRelease : released as
```

### Domain-model rules

- `InstrumentVersion`, `ResponseSnapshot`, `ResultSnapshot`, and published `ResearchRelease` are immutable semantic artifacts.
- `ScoringJob` is operational state; `ResultSnapshot` is scientific/product evidence. They are not the same aggregate.
- `ConsentSnapshot` records a purpose-specific decision and exact form/version evidence. Research consent is not inferred from service consent.
- `ResearchContribution` is a product-domain participation record; public research data uses a separate research participant namespace behind the restricted linkage boundary.
- Associations involving external scientific artifacts are references, not cross-service foreign keys into another service database.

## 2. Assessment-session state machine

```mermaid
stateDiagram-v2
    [*] --> Created
    Created --> Active: activate
    Active --> Paused: pause
    Paused --> Active: resume
    Active --> Completed: complete / freeze response snapshot
    Completed --> Scoring: begin_scoring
    Scoring --> Scored: record_score
    Scored --> Released: release

    Created --> Expired: expire
    Active --> Expired: expire
    Paused --> Expired: expire

    Created --> Cancelled: cancel
    Active --> Cancelled: cancel
    Paused --> Cancelled: cancel

    Created --> Invalidated: invalidate
    Active --> Invalidated: invalidate
    Paused --> Invalidated: invalidate
    Completed --> Invalidated: invalidate
    Scoring --> Invalidated: invalidate
    Scored --> Invalidated: invalidate

    Released --> [*]
    Expired --> [*]
    Cancelled --> [*]
    Invalidated --> [*]
```

Idempotent command replays may confirm an already-applied transition when the underlying evidence is identical. They must never rewind a later state or change frozen evidence.

## 3. Instrument-publication state machine

```mermaid
stateDiagram-v2
    [*] --> Draft
    Draft --> Review: submit_for_review
    Review --> Draft: request_revision
    Review --> Published: publish exact immutable version
    Published --> Suspended: suspend new sessions
    Suspended --> Published: reactivate same bytes/policy
    Published --> Retired: retire
    Suspended --> Retired: retire
    Retired --> [*]
```

A semantic content change after publication creates a new instrument version; it does not transition a published version back to Draft.

## 4. Data-rights state machine

```mermaid
stateDiagram-v2
    [*] --> Requested
    Requested --> IdentityVerified: verify requester for request
    IdentityVerified --> Processing: start durable operation
    Processing --> Completed: complete without retained scope
    Processing --> PartiallyCompleted: deletion + declared retained scope
    Requested --> Rejected: reject with evidence
    IdentityVerified --> Rejected: reject with evidence
    Processing --> Failed: terminal operation failure

    Completed --> [*]
    PartiallyCompleted --> [*]
    Rejected --> [*]
    Failed --> [*]
```

Export cannot complete with a deletion-retention exception. Exact terminal replays are idempotent; conflicting evidence for the same lifecycle reference fails closed.

## 5. Anonymous assessment happy-path sequence

```mermaid
sequenceDiagram
    autonumber
    actor P as Participant
    participant C as Reference Client
    participant A as Psychometrics Commons API
    participant DB as Product Store
    participant W as Product Worker
    participant F as fast-mlsirm
    participant N as Narrative Adapter

    P->>C: choose published instrument + locale
    C->>A: POST session (idempotency key)
    A->>DB: persist anonymous participant/session
    DB-->>A: session_ref + pinned instrument version
    A-->>C: session resource + item-delivery contract

    loop each response
        P->>C: answer item
        C->>A: submit response(client_event_ref)
        A->>DB: validate active state + append response event
        DB-->>A: server sequence / idempotent replay outcome
        A-->>C: accepted sequence
    end

    C->>A: complete session
    A->>DB: atomically state=Completed + freeze ResponseSnapshot + outbox scoring request
    A-->>C: completion accepted / scoring pending

    W->>DB: claim scoring work
    W->>F: version-pinned ScoringRequest
    F-->>W: scored/abstained/failed/excluded + provenance
    W->>DB: persist scoring evidence + immutable ResultSnapshot
    W->>N: render approved narrative from pinned ScoreProfile
    N-->>W: deterministic or validated optional-AI narrative
    W->>DB: attach narrative snapshot / release result

    C->>A: GET result
    A->>DB: authorize participant-owned result
    DB-->>A: immutable result snapshot
    A-->>C: scores + uncertainty + limitations + narrative provenance
```

## 6. Scoring dependency outage sequence

```mermaid
sequenceDiagram
    autonumber
    actor P as Participant
    participant A as Psychometrics Commons API
    participant DB as Product Store
    participant W as Product Worker
    participant F as fast-mlsirm scoring path

    P->>A: complete assessment
    A->>DB: commit Completed + immutable response snapshot + outbox
    A-->>P: completion durable; scoring pending

    W->>F: submit pinned scoring request
    F--xW: unavailable / retryable transport failure
    W->>DB: record typed retryable job failure + bounded retry schedule
    Note over DB: Response snapshot remains immutable and durable

    W->>F: retry same version-pinned request
    F-->>W: valid scoring result
    W->>DB: persist immutable result snapshot
```

No fallback score may be fabricated merely because the scoring dependency is unavailable.

## 7. Optional account-linking sequence

```mermaid
sequenceDiagram
    autonumber
    actor P as Participant
    participant C as Client
    participant A as Psychometrics Commons
    participant K as Keyverse
    participant DB as Product Store

    P->>C: choose to link anonymous history
    C->>K: authenticate / federate
    K-->>C: audience-bound identity assertion
    C->>A: link request + anonymous-session proof + Keyverse assertion
    A->>K: validate issuer/audience/signature/expiry/anti-replay context
    K-->>A: validated subject claims
    A->>DB: verify ownership and create append-only subject mapping
    DB-->>A: mapping evidence
    A-->>C: link complete

    Note over DB: Historical response/result identifiers are not rewritten
```

## 8. Research-contribution and release sequence

```mermaid
sequenceDiagram
    autonumber
    actor P as Participant
    participant A as Psychometrics Commons
    participant DB as Operational Store
    participant L as Restricted Linkage Boundary
    participant R as Research Staging
    participant S as semantic-data-portal

    P->>A: explicit research opt-in for versioned scope
    A->>DB: append consent evidence + research contribution
    DB-->>A: contribution_ref
    A->>L: create/reuse scoped research pseudonym
    L-->>A: research_participant_ref

    A->>R: build purpose-limited pseudonymized candidate snapshot
    R->>R: de-identification + rare-combination/privacy review
    R->>R: scientific/release approval
    R-->>A: immutable dataset snapshot + manifest digest

    A->>S: register release manifest + immutable artifact digests
    S-->>A: idempotent registration outcome

    Note over S: No Keyverse subject, operational participant ref, or linkage key in public release bundle
```

## 9. Modeling conventions

- Classes shown here are domain concepts, not a mandate that each concept map one-to-one to a Rust struct or database table.
- A sequence message is a contract responsibility, not evidence that the HTTP/event transport already exists.
- External systems are accessed through versioned adapters; direct database joins across bounded contexts are forbidden.
- Failure paths shown in the TRD remain normative even if omitted from a simplified happy-path diagram.

## 10. Reference

Object Management Group. (2017). *OMG Unified Modeling Language (OMG UML), Version 2.5.1*.
