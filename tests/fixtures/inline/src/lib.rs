//! Crate with inline diagrams.
//!
//! # Architecture
//!
#![doc = simple_mermaid::mermaid!("../docs/architecture.mmd")]
//!
//! # Flow
//!
//! The flow, rendered framed and left-aligned in rustdoc:
//!
#![doc = simple_mermaid::mermaid!("diagrams/flow.mmd" framed left)]
//!
//! # Usage
//!
//! Reference a diagram like this (this example must stay text):
//!
//! ```rust,ignore
//! #![doc = simple_mermaid::mermaid!("does/not/exist.mmd")]
//! ```
//!
//! # Again
//!
#![doc = mermaid!["../docs/architecture.mmd"]]
//!
//! The end.

/// Item docs are not crate docs.
#[doc = "not in the readme"]
pub fn item() {}
