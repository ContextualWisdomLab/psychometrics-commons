# ADR-0012: Exclude legacy R packages from product dependencies

- Status: Accepted
- Date: 2026-08-09
- Scope: `kaefa`, `aFIPC`, `nonnest2`, scientific validation and runtime dependencies

## Context

Earlier discussion considered using legacy R packages as runtime components, validation oracles, or candidates for relicensing. The necessary methods are being implemented in `fast-mlsirm` from primary literature and independent numerical specifications. Bringing the R packages into Psychometrics Commons would add licensing, runtime, packaging, parity, and duplicate-maintenance work without a product requirement.

## Decision

`kaefa`, `aFIPC`, and `nonnest2` are excluded from the Psychometrics Commons runtime, build, CI, container images, scientific oracle chain, and release dependency graph.

Required statistical methods are implemented and validated in `fast-mlsirm` using primary methodological sources, explicit equations, simulation, recovery, and independent test fixtures. Product work does not wait for relicensing these repositories.

## Invariants

1. Production and CI dependency manifests do not include these packages or an R runtime solely for them.
2. A result or model-selection decision cannot cite parity with one of these packages as its only validation.
3. New implementation documents primary sources and mathematical assumptions.
4. Recovery tests report bias, RMSE, coverage, convergence, and decision behavior rather than correlation alone.
5. Existing historical repositories remain separate and are not copied into the product under a new license without a rights audit.

## Validation strategy

- independent analytical fixtures for small cases;
- simulation under known true parameters;
- cross-backend and finite-difference gradient checks where applicable;
- published-example reproduction when source data and licensing permit;
- comparison against multiple independent implementations only as supplementary evidence.

## Alternatives rejected

- **Relicense and embed aFIPC/kaefa:** unnecessary for this product and may require contribution/ownership diligence.
- **Use nonnest2 as the authoritative Vuong oracle:** creates R coupling and does not replace formal model-specific validation.
- **Keep an optional R fallback:** doubles operational and scientific paths.

## Consequences

This reduces licensing and deployment complexity but places full responsibility for scientific traceability and validation on `fast-mlsirm`. Missing methods must be implemented rather than bypassed.

## Reversal conditions

A package may be reconsidered only if it supplies a uniquely necessary capability unavailable through a reasonable independent implementation, its rights and transitive dependencies are fully audited, and integration does not create a second source of truth. Such a change requires a superseding ADR.
