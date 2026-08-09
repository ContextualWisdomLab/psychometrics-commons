#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Domain primitives for the hosted Psychometrics Commons runtime.
//!
//! The crate owns product lifecycle semantics only. Psychometric numerical
//! computation remains in `fast-mlsirm` and is consumed through versioned
//! contracts rather than reimplemented here.

pub mod consent;
pub mod data_rights;
pub mod integration;
mod reference;
pub mod response;
pub mod result;
pub mod scoring;
pub mod session;
