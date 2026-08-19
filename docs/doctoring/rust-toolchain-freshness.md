# Rust toolchain freshness

Psychometrics Commons uses Rust `1.97.1` as its exact stable compiler baseline.
The project does not use a floating `stable` channel: a compiler transition is a
reviewed change that must preserve formatting, compilation, Clippy, tests,
rustdoc, PostgreSQL integration, and exact production coverage on one unchanged
head.

Branch coverage uses `cargo-llvm-cov` 0.8.6 with the reproducible
`nightly-2026-08-18` toolchain because upstream Rust branch coverage remains
unstable and nightly-only. Installation, tool verification, coverage generation,
and missing-branch diagnostics use the same date pin. Repository contract tests
reject both the predecessor `nightly-2026-08-01` value and inconsistent partial
updates.

GitHub Dependabot monitors the root `rust-toolchain.toml` through the
`rust-toolchain` package ecosystem. Stable compiler upgrades therefore arrive as
reviewable pull requests rather than silently changing CI behavior.

## References

GitHub. (2026). *Dependabot supports updates for Rust toolchains*. GitHub
Changelog. https://github.blog/changelog/

Rust Project Developers. (2026, July 16). *Announcing Rust 1.97.1*. Rust Blog.
https://blog.rust-lang.org/2026/07/16/Rust-1.97.1/

Taiki Endo and contributors. (2026). *cargo-llvm-cov* (Version 0.8.6)
[Computer software]. GitHub. https://github.com/taiki-e/cargo-llvm-cov
