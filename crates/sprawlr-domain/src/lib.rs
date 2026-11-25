//! The domain: an AWS account as a graph of resources and the connections between them, and
//! everything that can be derived from it without I/O (apps and environments, findings,
//! flows, capacity, cleanup, IAM reach, error patterns).
//!
//! Nothing here talks to AWS, the file system or a clock source beyond `chrono::Utc::now`
//! for evidence timestamps. Adapters feed it; use cases in `sprawlr-app` orchestrate it.

pub mod analysis;
pub mod changes;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
pub mod graph;
pub mod iam;
pub mod infer;
pub mod insights;
pub mod logs;
pub mod model;
pub mod status;

pub use graph::{ArchGraph, Snapshot};
pub use model::*;
