//! Re-export shim: the assertion-version provenance writer and the state-at
//! SPARQL builders now live in [`visionclaw_adapters::provenance_writer`] so CI
//! (which excludes this CUDA-linked crate) compiles and tests them. Existing
//! `crate::services::provenance_writer::*` paths are unchanged.
pub use visionclaw_adapters::provenance_writer::*;
