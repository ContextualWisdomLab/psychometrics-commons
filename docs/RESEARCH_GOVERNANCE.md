# Research Commons Governance

- Status: Normative product-governance baseline
- Date: 2026-08-09
- Product contribution/snapshot owner: `ContextualWisdomLab/psychometrics-commons`
- Catalog/release discovery owner: `ContextualWisdomLab/semantic-data-portal`

Research Commons turns explicitly contributed assessment/longitudinal data into reproducible, provenance-rich research releases. It is intentionally separated from normal product use so a participant can receive personal results without donating data to research.

## 1. Core principle

```text
service use != research participation
personal result != research release
operational participant identity != research participant identity
mutable operational database != immutable released dataset
```

Research contribution is explicit, scope-bound, versioned, revocable for future processing according to the stated policy, and absent by default.

## 2. Research lifecycle

```text
personal assessment/result
-> explicit research contribution consent
-> research contribution record
-> restricted operational-to-research pseudonym linkage
-> purpose-limited research staging projection
-> de-identification/privacy-risk review
-> scientific/data-quality review
-> immutable dataset snapshot
-> release approval
-> release manifest + immutable artifacts
-> semantic-data-portal registration
-> citation/access/audit lifecycle
```

A failure in the research path does not block a participant's personal result.

## 3. Consent and scope

A research contribution references exact evidence including:

```text
participant_ref (operational only)
consent_snapshot_ref
consent_form/version
research_scope_ref
allowed data categories / study or program scope
effective time
withdrawal state/evidence
```

A broad “improve services and research” checkbox is insufficient to authorize arbitrary future public release. Scope must be machine-readable enough that snapshot construction can fail closed when a requested variable/use is outside the participant's grant.

The consent user experience explains, as applicable:

- what types of data may be contributed;
- whether data may be public, controlled-access, or private/restricted;
- expected de-identification and residual re-identification risk;
- intended research scope;
- whether future approved research within a defined scope is permitted;
- withdrawal mechanics and limitations after an immutable release has already been published;
- contact/governance route for questions or data-rights requests.

## 4. Identity separation

Operational participant identifiers are not used as research identifiers.

A restricted linkage boundary maps an operational participant to a purpose/program-specific research pseudonym. The mapping:

- is unavailable to ordinary analytics and public-release workflows;
- is accessed only through an explicit restricted role/purpose;
- is encrypted and audited;
- records pseudonym/linkage-key version;
- is never included in release artifacts or catalog metadata.

Public or controlled release bundles must not contain:

- Keyverse subject identifiers;
- `assessment_participant_ref` or other operational participant references;
- restricted linkage identifiers/keys;
- service authentication identifiers/tokens;
- internal object-store or database credentials/locations that bypass access policy.

Before packaging a public fixture, call `scan_public_release_fixture`.

- Give the scanner the columns that the fixture will publish. Column names must use the service's ASCII schema grammar; non-ASCII aliases fail closed.
- Give the scanner the product-authorized restricted-identity inventory for the represented people.
- The scanner rejects governance/product identity columns, restricted linkage-key aliases, authentication/credential/internal-location columns, structured cells it cannot inspect safely, and exact restricted identity values.
- Credential matching is intentionally conservative: normalized credential markers such as `token`, `secret`, `password`, and `credential` fail closed even inside a longer column word such as `tokenized_score`; a credential-shaped prefix such as `key_research_participant_ref` also fails closed. Rename a benign alias or establish a separately reviewed contract instead of weakening this boundary ad hoc.
- If every restricted-identity inventory category is empty or blank, the scanner fails closed. Missing inventory is not clean release evidence.
- The separately governed `research_participant_ref` namespace remains allowed unless a restricted-identity or credential namespace is prepended to it.

This product boundary must not query Keyverse, a linkage service, or another service's application database to supplement missing inventory.

## 5. Research staging projection

A research snapshot is built from an explicit variable projection rather than a raw operational table dump.

The projection records:

```text
research_scope_ref
source product/resource versions
included variable definitions
transform/derivation versions
missingness/quality policy
participant/contribution eligibility policy
pseudonymization version
snapshot query/build artifact digest
```

Variables not allowlisted by scope are absent, not merely visually masked.

Free-text/open-ended responses are excluded by default from broadly accessible releases because they can contain direct or contextual identifying information. Inclusion requires a separately reviewed scientific need, consent compatibility, disclosure-risk process, access class, and transformation policy.

## 6. Privacy-risk review

Privacy review considers more than direct identifiers.

Review dimensions include, as appropriate:

- rare/small subgroup combinations;
- exact dates/times or temporal trajectories;
- geography/organization/project combinations;
- unique longitudinal patterns;
- high-dimensional demographics/context;
- free text and quoted source content;
- linked external/public datasets that increase joinability;
- very small cells after stratification;
- repeated releases that can be differenced to infer suppressed information.

The product does not promise “anonymous” data merely because direct identifiers were removed. Release documentation states the actual transformation/access model and known residual risks.

## 7. Scientific/data-quality review

Before release, verify:

- exact instrument/item versions;
- scoring/calibration/norm versions for derived scores;
- locale/translation evidence;
- construct and variable definitions;
- missingness and exclusion rules;
- unit/range/category definitions;
- sample construction and time window;
- known selection/consent bias;
- DIF/invariance limitations relevant to intended comparisons;
- longitudinal timing/context fields when present;
- codebook/variable-dictionary consistency with actual data;
- deterministic/reproducible snapshot generation from approved source state when retention policy permits reconstruction.

Derived psychometric scores are never presented as raw observed variables without their scoring/provenance context.

## 8. Immutable dataset snapshot

A dataset snapshot is identity-bearing only when its artifacts and manifest are frozen.

At minimum it records:

```text
dataset_snapshot_ref
manifest_digest
artifact digests
schema/codebook/variable dictionary digests
source instrument/item/scoring/calibration/norm refs
consent/research scope
privacy review ref
scientific review ref
snapshot creation time
access class candidate
supersedes/superseded-by relation if applicable
```

Corrections do not mutate published bytes under the same release identity. A corrected snapshot/release explicitly supersedes the prior one and states the correction.

## 9. Release bundle

A standard release bundle contains, as applicable:

```text
dataset.parquet
dataset.csv
codebook.json
variable_dictionary.json
data_card.md
license_record.json
consent_scope.json
instrument_version.json
item_version_manifest.json
scoring_version.json
calibration_reference.json
norm_version.json
privacy_review.json
scientific_review.json
citation.cff
checksums.sha256
```

The bundle format may evolve through a versioned manifest schema. A missing optional artifact is documented; a required artifact cannot be silently omitted.

## 10. Access classes

Research releases explicitly declare access class:

- **public** — artifacts are intended for unrestricted public retrieval under the release license/policy;
- **controlled** — catalog metadata may be public, but artifact access requires an authorization decision and durable access evidence;
- **private/internal** — visible only to explicitly authorized research/product roles;
- **embargoed** — approved but unavailable until a defined release condition/time.

Moving between access classes is a governed release action. A restricted release cannot leak direct artifact URLs or credentials through catalog metadata.

## 11. semantic-data-portal boundary

Psychometrics Commons creates/approves the immutable research release manifest and supplies artifact references/digests through the integration contract.

`semantic-data-portal` owns catalog, ontology/lineage/discovery, release presentation, and its own access-control implementation. It does not query Psychometrics Commons operational response tables.

Registration is idempotent by release identity and manifest digest. Reusing a release reference with a different digest fails closed.

## 12. Citation and provenance

Every released dataset has stable citation metadata and version identity. Citation should allow a researcher to identify the exact dataset release used, not merely a mutable project landing page.

Release provenance connects, without leaking participant identity:

```text
research release
-> dataset snapshot
-> variable/codebook/data-card versions
-> instrument/item versions
-> scoring/calibration/norm artifacts
-> privacy/scientific approval evidence
-> license/consent scope
```

## 13. Withdrawal and immutable releases

Withdrawal affects future contribution processing and future release eligibility according to the applicable scope, law, and disclosed policy.

The product must not promise that a participant can retroactively remove every copy of an already public immutable dataset if that promise cannot actually be fulfilled. The consent/release policy must state what happens after publication and what mechanisms—new release exclusion, withdrawal notice, controlled-access revocation, downstream notice, or other action—are realistically available.

Operational systems preserve withdrawal evidence so a later snapshot does not accidentally re-include the contribution.

## 14. Dataset correction and retraction

A release can require correction or retraction because of:

- privacy leak or re-identification risk;
- consent/scope error;
- scientific/scoring error;
- corrupted artifact or schema mismatch;
- licensing/right issue;
- material provenance error.

Correction creates a superseding release. Retraction keeps enough durable catalog/audit metadata to tell users that a release should no longer be used while limiting exposure of problematic artifacts according to the incident policy.

A catalog page cannot silently replace old data bytes while preserving the same release identity/checksum.

## 15. Research reproducibility

A release should allow an independent researcher to understand and, where legally/operationally possible, recreate derived variables and scores from the declared inputs.

Reproducibility evidence includes:

- immutable data/metadata digests;
- scoring/measurement versions;
- code/transform versions or published methods;
- exact variable definitions;
- sampling/exclusion/missingness rules;
- temporal and locale context;
- known limitations;
- software/environment provenance when material to derived outputs.

## 16. Measurement limitations in research use

Research Commons must not imply that publication turns a measure into a universally invariant construct.

Data cards/codebooks disclose:

- population and administration context;
- instrument locale/version;
- norm/calibration population where derived scores are supplied;
- known or untested DIF/invariance conditions;
- reliability/scoreability limitations;
- whether repeated measures can be interpreted longitudinally;
- any sampling/consent selection that limits generalization.

## 17. Open-science principle

Where participant rights, licenses, privacy, and scientific quality permit, releases should be easy to find, understand, cite, and reuse through machine-readable metadata and documented access terms. “Open” is not interpreted as bypassing privacy or consent boundaries.

The product favors stable schemas, explicit licenses, variable dictionaries, data cards, checksums, citation metadata, and provenance so research reuse does not depend on tribal knowledge.

## 18. Release validation tests

As implementation grows, tests include:

- non-opted-in contribution cannot enter snapshot;
- withdrawn contribution excluded from future eligible snapshots;
- operational/Keyverse/linkage identifiers fail release validation;
- variable outside consent/research scope fails closed;
- manifest digest/release-ID conflict rejected;
- codebook/variable dictionary matches actual columns/types/categories;
- checksums verify actual artifact bytes;
- scoring/norm/instrument references resolve to immutable known versions;
- public/controlled/private access behavior matches release class;
- portal outage queues/reconciles without affecting personal results;
- superseding release preserves old release identity and correction relation;
- backup/restore does not re-enable a withdrawn contribution for future release.

## 19. Research release gate

A release is blocked until the exact snapshot has:

- valid explicit research scope;
- privacy-risk review;
- scientific/data-quality review;
- complete required metadata bundle;
- license/right compatibility;
- exact measurement provenance for derived scores;
- access-class approval;
- immutable manifest/artifact digests;
- citation metadata;
- release approver evidence distinct from ordinary identity administration;
- no unresolved release-blocking security/privacy/scientific finding.

## 20. Related architecture artifacts

- `docs/architecture/ERD.md` — research identity linkage, snapshot members, releases
- `docs/architecture/SECURITY_AND_DATA.md` — trust/data classification and privacy risk
- `docs/architecture/UML.md` — research-contribution/release sequence
- `docs/adr/0006-consent-data-rights-and-research-separation.md`
- `docs/adr/0007-semantic-data-portal-research-release-boundary.md`
- `docs/adr/0010-versioned-provenance-and-immutable-results.md`
