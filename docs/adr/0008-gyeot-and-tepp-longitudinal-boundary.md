# ADR-0008: Gyeot and TEPP longitudinal boundary

- Status: Accepted
- Date: 2026-08-09
- Scope: EMA/ESM collection, offline sync, temporal semantics, event and multiple-membership analysis

## Context

Longitudinal self-understanding requires mobile momentary collection, offline operation, event-time semantics, and models that distinguish within-person change from between-person differences. Duplicating collection in TEPP or temporal modeling in the product runtime would create inconsistent time semantics and atomistic analyses.

## Decision

Gyeot owns the participant-facing EMA/ESM and JITAI collection experience, including offline-first local observations and sync. Psychometrics Commons owns authorization, program enrollment, consent, and normalized ingestion. TEPP owns temporal/event/relationship, multilevel, cross-classified, and multiple-membership analytical artifacts.

The hosted runtime does not implement DSEM, continuous-time, event ontology, or longitudinal ESEM kernels. TEPP does not own participant sessions or mobile synchronization.

## Observation time contract

Each longitudinal observation preserves distinct timestamps where applicable:

- `observed_at`: when the participant reports the state/event;
- `recorded_at`: when the client stored it;
- `received_at`: when the server accepted it;
- `available_at`: when it became analytically available;
- `valid_from` / `valid_to`: interval validity for referenced context.

Server receipt time never silently replaces observed time. Timezone and offset are preserved, and normalization to UTC retains the original civil-time context.

## Context and membership

Observations may reference multiple organizations, projects, relationships, or contexts. Multiple-membership weights are explicit, validated, and versioned; they are not collapsed to a single primary group merely for database convenience.

## Offline synchronization

Client observations have stable client references and content digests. Sync is idempotent. Conflicting edits produce an explicit conflict record or superseding observation; there is no blind last-write-wins for clinically or scientifically meaningful data.

## Invariants

1. Longitudinal participation requires separate valid consent.
2. Offline storage is encrypted using platform capabilities and excludes unnecessary identity data.
3. Event-time fields are immutable after acceptance except through audited correction/supersession.
4. Within-person and between-person effects are not conflated in published analyses.
5. TEPP artifacts reference exact input snapshot and model version.
6. Multiple-membership and time-varying context are preserved when declared by the study design.

## Failure modes

Sync outages leave bounded local queues and clear user state. Clock anomalies are flagged rather than silently reordered. TEPP failure does not delete observations; analysis jobs are retryable and artifacts are immutable by input digest.

## Validation

- offline/reconnect/idempotency tests;
- timezone, daylight-saving, and clock-skew tests;
- multiple-membership validation and recovery simulations;
- within/between decomposition tests;
- TEPP artifact provenance contract tests.

## Alternatives rejected

- **Psychometrics Commons implements all temporal models:** duplicates TEPP and expands the runtime beyond product orchestration.
- **TEPP collects mobile observations directly:** couples modeling to client lifecycle.
- **One timestamp and one group per observation:** scientifically invalid for the intended designs.

## As-built versus target

Active PR #226 adds the product enrollment primitive in `src/longitudinal.rs`. It is `IMPLEMENTED_ON_ACTIVE_PR`, not protected-main truth, until the exact reviewed head is merged. Do not treat #184 as the landing vehicle; that head authorized collection from enrollment state alone. Do not treat #199 as the landing vehicle; that head still authorized collection from a caller-supplied enroll-time snapshot.

As-built on this PR:

- enrollment requires a tenant-owned `ParticipantRecord` plus an active `ConsentPurpose::LongitudinalObservation` grant and fails closed when that grant is missing or revoked;
- `authorize_collection` re-checks the current consent ledger and the tenant-owned `ParticipantRecord`, so a later revoke or a second clinic fails closed even if the enrollment is still `Enrolled` and the enroll-time snapshot still looks granted;
- research refusal does not block personal EMA/ESM enrollment;
- work/home and other membership contexts stay distinct and reject duplicates;
- pause, resume, and withdraw are fail-closed and do not erase enrollment evidence.

Still target:

- PostgreSQL enrollment/observation persistence;
- live Gyeot collection and TEPP analysis adapters;
- HTTP enrollment transport;
- observation-time fields (`observed_at`, `recorded_at`, `received_at`, `available_at`, `valid_from` / `valid_to`) on ingested records.

## References

Bolger, N., & Laurenceau, J.-P. (2013). *Intensive longitudinal methods: An introduction to diary and experience sampling research*. Guilford Press.

Curran, P. J., & Bauer, D. J. (2011). The disaggregation of within-person and between-person effects in longitudinal models of change. *Annual Review of Psychology, 62*, 583–619. https://doi.org/10.1146/annurev.psych.093008.100356

Diez Roux, A. V. (2002). A glossary for multilevel analysis. *Journal of Epidemiology & Community Health, 56*(8), 588–594. https://doi.org/10.1136/jech.56.8.588

Hamaker, E. L., & Wichers, M. (2017). No time like the present: Discovering the hidden dynamics in intensive longitudinal data. *Current Directions in Psychological Science, 26*(1), 10–15. https://doi.org/10.1177/0963721416666518

## Reversal conditions

Collection or analysis implementations may change if their contracts remain stable. Revisit the boundary only if one component ceases independent use and the combined ownership demonstrably reduces rather than increases coupling.
