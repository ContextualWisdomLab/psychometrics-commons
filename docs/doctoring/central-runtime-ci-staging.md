# Central Runtime CI staging

The PR-only caller pins the central fixed-consumer callee to
`29c14c67fd2a49e3719bfc0d3c1b9cad9531e33f`. It runs only when the operator sets
`PSYCHOMETRICS_ISOLATED_CI_ADMITTED=true` after validating disposable execution
and repository admission. The variable is not set by this change. Skipping the
caller does not establish successful Runtime CI.

The original Runtime CI, SPDX SBOM and source provenance files remain intact.
Their hosted workflow registrations were stopped under the owner's explicit
infrastructure instruction after exact-head annotations reported a billing
lock before any runner or step executed. Their failed historical checks remain
failed. SBOM, package provenance, CodeQL security and product acceptance are not
waived or represented as passed.

The generated optional Code Quality feature was set to `not-configured` through
`/repos/ContextualWisdomLab/psychometrics-commons/code-quality/setup`; generic
workflow disable returned 422. Security CodeQL default setup was not changed.

PR #165 received genuine Noema App approval on exact head
`bb9028714d996871ed2588ea9a3aa83fd581af84` in central self-hosted run
`38398698979`, using the configured personal LiteLLM `auto` route. The review
explicitly limits itself to cited source paths in truncated context and did not
execute tests. It is independent review evidence, not full validation.

Local caller checks: two unittest cases pass; actionlint passes. The full Python
contract suite at product main `e81441ce` reports one existing provenance
concurrency mismatch introduced by #446; no source-package contract threshold
was weakened to hide that failure. Actual callee execution remains pending.
