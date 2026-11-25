//! In-memory adapter for the fictional demo account (`sprawlr_domain::fixtures`). It serves
//! the same ports as a real account, so the demo exercises the same use cases.

use crate::ports::{Discovered, ErrorLogSource, HealthSource, InfraSource, Progress, Warnings};
use sprawlr_domain::{fixtures::demo_graph, ArchGraph};

/// The demo account analysed exactly as a scan would be (IAM reach, inference, analysis),
/// for drivers that need a snapshot without running the async use case.
pub fn snapshot() -> sprawlr_domain::Snapshot {
    let mut g = demo_graph();
    sprawlr_domain::iam::apply(&mut g);
    sprawlr_domain::infer::infer(&mut g);
    g.snapshot()
}

/// The demo account. Its health and error logs are part of the fixture, so reading them
/// again changes nothing.
pub struct Demo;

impl InfraSource for Demo {
    async fn discover(&self, progress: &Progress) -> anyhow::Result<Discovered> {
        progress("reading the demo account".into());
        Ok(Discovered { graph: demo_graph(), warnings: vec![] })
    }
}

impl HealthSource for Demo {
    async fn read_health(&self, _: &mut ArchGraph, _: &Progress) -> anyhow::Result<Warnings> {
        Ok(vec![])
    }
}

impl ErrorLogSource for Demo {
    async fn read_errors(&self, _: &mut ArchGraph, _: &Progress) -> anyhow::Result<Warnings> {
        Ok(vec![])
    }
}
