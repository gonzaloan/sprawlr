//! Application layer: ports (what sprawlr needs from outside) and use cases (what a person
//! asks it to do). Depends only on `sprawlr-domain`.
//!
//! ```text
//! drivers (cli, desktop, mcp) ──▶ use cases ──▶ ports ◀── adapters (aws, fs, demo)
//!                                     │
//!                                     ▼
//!                                  domain
//! ```

#[cfg(any(test, feature = "demo"))]
pub mod demo;
pub mod ports;
pub mod usecases;

pub use ports::{Discovered, ErrorLogSource, HealthSource, InfraSource, Progress, SnapshotStore, Warnings};
pub use usecases::{refresh_health, scan_account, ScanOptions, ScanResult};
