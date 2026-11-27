//! Application layer: ports (what sprawlr needs from outside) and use cases (what a person
//! asks it to do). Depends only on the pure crates: `sprawlr-domain` and `sprawlr-iac`.
//!
//! ```text
//! drivers (cli, desktop, mcp) ──▶ use cases ──▶ ports ◀── adapters (aws, fs, demo)
//!                                     │
//!                                     ▼
//!                                  domain
//! ```

#[cfg(any(test, feature = "demo"))]
pub mod demo;
#[cfg(any(test, feature = "demo"))]
mod demo_config;
pub mod export;
pub mod ports;
pub mod queries;
pub mod settings;
pub mod usecases;

pub use export::{export_code, CodeExport};
pub use ports::{
    ConfigSource, Discovered, ErrorLogSource, HealthSource, InfraSource, Notifier, Progress, SettingsStore, SignInNeeded, SnapshotStore,
    Warnings,
};
pub use settings::{next_wait, DesktopSettings, Every, Schedule};
pub use usecases::{refresh_health, refresh_in_background, scan_account, Background, RefreshOptions, ScanOptions, ScanResult};
