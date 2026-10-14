## Measurement coordinate provenance

- Add a versioned, participant/source-text-free `MeasurementCoordinateProvenance` contract for released longitudinal consumers. The authority binds assessment specification, instrument/scoring/calibration/norm provenance, scoring-output schema, canonical engine digest, and exact scored construct identity without carrying or recomputing the numeric score.
- Add strict canonical decode→re-encode validation so malformed lengths, aliases, unsupported versions, invalid references, trailing fields, and non-canonical engine digests fail closed.
- Bound the v1 canonical cross-repository envelope at 8 KiB. Oversized consumer bytes are rejected before UTF-8 parsing and owner projection preflights encoded size before cloning immutable snapshot provenance; provenance is never truncated.
- Keep the contract release-gated: mutable branch/PR heads are development evidence only, and downstream TEPP#602 may consume it only from a protected-main immutable release with exact-head CI/security/privacy/coverage/review and supply-chain evidence.
