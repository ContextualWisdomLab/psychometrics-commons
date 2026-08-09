# ADR-0007: Research-release boundary with semantic-data-portal

- Status: Accepted
- Date: 2026-08-09
- Scope: research snapshots, release approval, catalog, provenance, public/controlled distribution

## Context

Psychometrics Commons owns operational responses and research-contribution decisions. `semantic-data-portal` owns catalog, ontology, lineage, license, discovery, and release presentation. Sharing a database or letting the catalog query operational responses would violate bounded ownership and make releases mutable.

## Decision

Psychometrics Commons creates an immutable, reviewed `dataset_snapshot` and submits a release manifest to `semantic-data-portal`. The portal registers and serves approved release metadata and artifacts but never reads the operational assessment database.

## Required release bundle

A release manifest references immutable artifacts including, as applicable:

- Parquet and CSV data;
- codebook and variable dictionary;
- data card and known limitations;
- license record and consent scope;
- exact instrument, item, scoring, calibration, and norm versions;
- privacy-risk review and approval evidence;
- citation metadata;
- cryptographic checksums;
- supersedes/superseded-by relations.

Artifact bytes may live in an approved object store; the manifest contains digests and resolvable locations, not mutable `latest` paths.

## Workflow

```text
snapshot_requested -> building -> privacy_review -> scientific_review
-> approved -> registered -> published
```

Terminal alternatives: `rejected`, `withdrawn_before_publication`, `superseded`.

Portal registration is idempotent by release identifier and manifest digest. A digest mismatch for an existing identifier fails closed.

## Invariants

1. A release is derived from one immutable dataset snapshot.
2. Portal metadata cannot change the underlying data or scoring provenance.
3. Corrections create a new release that supersedes the old; no in-place dataset replacement.
4. Operational identifiers and linkage keys are prohibited.
5. License and consent scope are machine-readable and human-readable.
6. Catalog availability is not required to retrieve a participant's personal result.

## Failure modes

Portal outage queues registration through an outbox. Artifact upload failure leaves the release unpublished. Partial registration is reconciled by digest and idempotency, never by assuming last write wins.

## Security and access

Public, controlled-access, and private releases are distinct policies. Controlled access requires an authorization decision at download time and a durable access record. A public catalog record must not expose restricted artifact URLs or object-store credentials.

## Validation

- manifest JSON Schema and checksum verification;
- reproducible rebuild from snapshot and version bundle;
- negative identifier-leak tests;
- idempotent registration and partial-failure tests;
- controlled-access authorization and audit tests.

## Alternatives rejected

- **Portal queries operational DB:** violates least privilege and makes snapshots non-reproducible.
- **Upload only a CSV:** insufficient provenance, licensing, and interpretation.
- **Edit a published release in place:** breaks reproducibility and citation.

## Reversal conditions

The artifact store or portal implementation may change while preserving immutable manifests, digest identity, and no-direct-database-access rules.
