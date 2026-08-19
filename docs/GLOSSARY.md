# Psychometrics Commons Glossary

- Status: Normative terminology baseline
- Date: 2026-08-09

Use these terms consistently across PRD, TRD, ADRs, APIs, diagrams, code, UI, and research releases. A term can have a more precise implementation type, but its product meaning must not silently drift.

| Term | Meaning in Psychometrics Commons |
|---|---|
| **AssessmentSpec** | Domain-neutral measurement/scoring specification owned by fast-mlsirm and referenced by the product. Not a product session or instrument publication record. |
| **construct** | The latent attribute/capacity/stance/state that an instrument is intended to measure under a documented interpretation/use. |
| **instrument definition** | Stable conceptual identity of an assessment instrument family. |
| **instrument version** | Immutable locale/content/scoring-provenance-specific published or pre-publication form of an instrument. A semantic content change creates a new version. |
| **item definition** | Stable conceptual item identity where item lineage is tracked. |
| **item version** | Immutable presentation/response-schema-specific item content/version. |
| **instrument release** | Product-owned publication record that binds an exact instrument version/item set and scientific/policy references to a publication lifecycle. |
| **assessment participant** | Product-domain participant identity. It may be anonymous or optionally linked to a Keyverse subject. It is not a public research identifier. |
| **anonymous participant** | Participant using a short-lived/product-scoped identity without a Keyverse account. Anonymous does not mean the assessment data are automatically research-anonymous. |
| **assessment session** | Server-authoritative lifecycle resource for one administration of one pinned instrument version to one product participant. |
| **response event** | Immutable accepted response submission event with client idempotency reference and server ordering. |
| **response snapshot** | Immutable frozen set/prefix of accepted response events created at completion and used as scoring evidence. |
| **scoring job** | Operational durable work resource requesting a score from a pinned response/measurement/version bundle. It is not itself the scientific result. |
| **ScoreProfile** | Versioned structured score output from the scientific scoring contract used as the source for result presentation/narrative. |
| **result snapshot** | Immutable product result/provenance artifact binding response snapshot, scoring output, instrument/scoring/calibration/norm/narrative/engine evidence. Corrections/rescoring supersede rather than mutate. |
| **Personality Style** | Product presentation/narrative mapping derived from continuous/facet profile. It is not a psychometric type score and does not claim MBTI equivalence. |
| **style assignment key** | Deterministic digest identity binding all behavior-affecting score/instrument/scoring/norm/mapping/rule/locale inputs used to select a Personality Style. Optional AI wording provenance is separate. |
| **reflection construct** | Independently measured construct used for reflection, such as self-compassion when rights/validation permit. It is not inferred from Big Five by default. |
| **consent form** | Versioned content/policy defining a specific processing purpose and participant decision surface. |
| **consent snapshot** | Immutable evidence of one participant's purpose-specific decision under an exact consent-form/scope version. |
| **research contribution** | Explicit product-domain opt-in record that makes approved data potentially eligible for research processing under a defined scope. Service use alone does not create one. |
| **research participant** | Pseudonymous research-domain identity separated from operational participant identity through a restricted linkage boundary. |
| **restricted linkage** | Highly restricted mapping between operational participant identity and research pseudonym identity. Never part of a public release. |
| **research staging** | Purpose-limited pseudonymized data projection used for privacy/scientific review before a dataset snapshot is approved. |
| **dataset snapshot** | Immutable reviewed research dataset/artifact set prepared for a release. Distinct from mutable staging or operational tables. |
| **research release** | Immutable access-classed published/registered dataset release with manifest, provenance, consent scope, license, privacy/scientific review, citation and checksums. |
| **Research Commons** | Product experience/workflow for governed research contribution and reusable data releases. Catalog/discovery is owned by semantic-data-portal. |
| **data-rights request** | Durable export/deletion lifecycle resource with requester verification, scope, operation and completion/retention evidence. |
| **Keyverse subject** | Identity-provider/federation subject owned by Keyverse. It is an external identity reference, not a product authorization decision or research identifier. |
| **tenant** | Product authorization/data-isolation scope. A tenant identifier never substitutes for an authorization check. |
| **bounded context** | Independently owned domain with its own source of truth and contracts. Cross-context integration uses APIs/events/artifacts, not normal direct DB access. |
| **system of record (SoR)** | Bounded context that authoritatively owns a fact/resource. A cached/reference copy does not become SoR. |
| **transactional outbox** | Product-owned durable event record written in the same local transaction as the state change whose downstream effects it represents. |
| **integration inbox** | Consumer-side durable deduplication/processing evidence for at-least-once delivered events. Receipt is not equivalent to completion of a required side effect. |
| **idempotency key** | Caller/resource-scoped identity allowing exact request replay to return the original outcome while conflicting reuse fails closed. Not a general cache key. |
| **content digest** | Cryptographic digest of canonical bytes used to establish immutable artifact identity/integrity in addition to opaque resource references. |
| **provenance** | Exact references/digests/versions required to explain/replay the origin and transformations of a score, result, artifact, or release. |
| **supersession** | Explicit relationship from a newer immutable artifact/result/release to an older one. It does not mutate or erase the prior artifact. |
| **DIF** | Differential Item Functioning: conditional item behavior differences across groups/contexts that require psychometric investigation, not merely observed mean differences. |
| **measurement invariance** | Evidence about whether intended construct/measurement relations support the proposed comparisons across groups/locales/time/modes. |
| **scoreability** | Evidence that a fitted measurement structure supports interpreting/reporting the proposed general/subscale score. Good model fit alone is not sufficient. |
| **testlet** | Shared-stimulus/local-dependence grouping modeled as nuisance or structured dependence rather than automatically treated as a substantive trait. |
| **multifactor** | Measurement structure with multiple substantive latent traits. Distinct from multifaceted. |
| **multifaceted** | Measurement structure accounting for systematic facets such as rater/task/occasion effects. Distinct from multiple substantive traits. |
| **multiple membership** | Observation/person belongs to multiple relevant higher-level contexts with explicit membership representation rather than one forced primary group. |
| **validity time** | Interval when the reported state or event was true. A point observation uses the same instant for start and end. Receipt time must not replace it. |
| **recorded time** | Instant the collection client stored the observation, including offline local storage. |
| **received time** | Instant Psychometrics Commons first accepted the candidate at its trust boundary. |
| **ingested time** | Instant Commons durably accepted the normalized observation row. |
| **within-person change** | Variation over time within the same participant. Must not be conflated with between-person differences. |
| **AI narrative** | Optional bounded prose rendering from pinned product/scientific evidence. It is not allowed to modify the source scientific result. |
| **LLM judge** | LLM producing a rating/criterion observation. It is treated as a fallible rater with possible severity/bias/drift, not ground truth by default. |
| **deterministic fallback** | Approved non-LLM output path that preserves core product behavior when optional AI is unavailable/invalid/denied. |
| **Community profile** | Minimal standalone/community-research deployment with product runtime/store, fast-mlsirm-compatible scoring and a simple client; optional CWL integrations can be absent. |
| **Hosted profile** | CWL-operated deployment enabling selected CWL bounded contexts as independently observable capabilities. |
| **Enterprise profile** | Customer/self-hosted or contracted deployment adding federation/residency/retention/encryption/network/provider/operations policy while preserving the same domain contracts. |
| **SLO** | Measured service-level objective for a defined capability/profile/indicator/window. No universal value is assumed in architecture docs. |
| **RPO** | Recovery Point Objective for a defined data domain/profile, backed by actual backup/recovery design/evidence. |
| **RTO** | Recovery Time Objective for a defined data domain/capability/profile, backed by measured recovery evidence. |
| **target architecture** | Normative intended semantics/components/contracts that may not yet be implemented. |
| **as-built** | Behavior/topology/schema/contract actually implemented and verified on a named protected-main/release baseline. |
| **GA evidence** | Integrated exact-release evidence demonstrating required product/scientific/security/privacy/accessibility/operational/recovery gates; not inferred from document completeness. |
