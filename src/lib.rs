#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Domain primitives for the hosted Psychometrics Commons runtime.
//!
//! The crate owns product lifecycle semantics only. Psychometric numerical
//! computation remains in `fast-mlsirm` and is consumed through versioned
//! contracts rather than reimplemented here.

pub mod anonymous_session;
pub mod authorization;
pub mod consent;
pub mod data_rights;
pub mod deterministic_narrative;
pub mod health;
pub mod instrument;
pub mod integration;
pub mod item_delivery;
pub mod narrative;
pub mod participant;
pub mod postgres_consent;
pub mod postgres_data_rights;
pub mod postgres_health;
pub mod postgres_inbox_consumption;
pub mod postgres_instrument_release;
pub mod postgres_integration;
pub mod postgres_response_snapshot;
pub mod postgres_scoring_job;
pub mod postgres_scoring_request;
mod reference;
pub mod research_release;
pub mod response;
pub mod result;
pub mod scoring;
pub mod scoring_job;
pub mod session;
