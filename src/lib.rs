#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Domain primitives for the hosted Psychometrics Commons runtime.
//!
//! The crate owns product lifecycle semantics only. Psychometric numerical
//! computation remains in `fast-mlsirm` and is consumed through versioned
//! contracts rather than reimplemented here.

pub mod account_link;
pub mod anonymous_authorization;
pub mod anonymous_credential;
pub mod anonymous_session;
pub mod api_problem;
pub mod audit;
pub mod authorization;
pub mod consent;
pub mod data_rights;
pub mod data_rights_authorization;
pub mod deterministic_narrative;
pub mod health;
pub mod instrument;
pub mod integration;
pub mod integration_delivery;
pub mod integration_publisher;
pub mod item_delivery;
pub mod localized_result_report;
pub mod longitudinal_observation;
pub mod narrative;
pub mod participant;
pub mod postgres_assessment_session;
pub mod postgres_audit;
pub mod postgres_audit_retention;
pub mod postgres_consent;
pub mod postgres_data_rights;
pub mod postgres_data_rights_completion;
pub mod postgres_data_rights_processing;
pub mod postgres_health;
pub mod postgres_inbox_consumption;
pub mod postgres_instrument_release;
pub mod postgres_integration;
pub mod postgres_item_delivery;
pub mod postgres_response_snapshot;
pub mod postgres_result_snapshot;
pub mod postgres_scoring_job;
pub mod postgres_scoring_request;
mod reference;
pub mod research_release;
pub mod response;
pub mod result;
pub mod result_authorization;
pub mod result_export;
pub mod result_export_authorization;
pub mod scoring;
pub mod scoring_engine;
pub mod scoring_job;
pub mod session;
#[path = "session_http_boundary.rs"]
pub mod session_http;
pub mod style_mapping;
