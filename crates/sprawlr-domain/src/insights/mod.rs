//! Insights: what the graph means for the people running it.
//!
//! - **Flows**: paths requests and events take, from an entry point to where they end,
//!   split into stages at every asynchronous hop.
//! - **Capacity**: utilisation of each resource at its busiest hour, and the traffic
//!   multiplier at which it saturates (what-if).
//! - **Risks and recommendations** that need flow context: shared critical dependencies,
//!   long synchronous chains, where errors start, missing dead-letter queues, timeouts
//!   that do not fit together.
//! - **Lifecycle**: idle, orphaned and zombie resources, with what they cost.
//!
//! Everything is deterministic and derived from the graph; each item says what it rests on.
//!
//! Layout: `signals` reads metrics off nodes, `flows` walks the graph, `capacity` measures
//! headroom, `rules` turns flows and capacity into recommendations, `lifecycle` finds what
//! is probably not needed. Types shared by all of them live here.

mod capacity;
mod flows;
mod lifecycle;
mod rules;
mod signals;
#[cfg(test)]
mod tests;

pub use capacity::{capacity, fmt_x};
pub use flows::flows;
pub use lifecycle::lifecycle;
pub use rules::insights;

use crate::{ArchGraph, EdgeKind, NodeId};
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Reached from the internet (CloudFront, public API, internet-facing load balancer).
    Internet,
    /// A data store emits events: S3 notifications, DynamoDB streams.
    Event,
    /// Starts on its own: a schedule, a manual run, or a caller sprawlr cannot see.
    SelfStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Sync,
    Async,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hop {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub mode: Mode,
    /// Known only from IAM permissions (the resource shows nothing else).
    #[serde(default)]
    pub inferred: bool,
    /// 0 for the synchronous part that answers the caller; +1 after every async hop.
    pub stage: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub name: String,
    pub app: Option<String>,
    pub env: Option<String>,
    pub trigger: Trigger,
    pub entry: NodeId,
    /// Nodes in the order a request reaches them.
    pub nodes: Vec<NodeId>,
    pub hops: Vec<Hop>,
    pub stages: usize,
    /// Executions in the last 24 h, from the entry point's traffic metric.
    pub runs_24h: Option<f64>,
    /// Errors in the last 24 h across the flow's resources.
    pub errors_24h: f64,
    /// Longest synchronous chain from the entry point (number of hops).
    pub sync_depth: usize,
    /// Sum of p95 latencies along that chain.
    pub sync_latency_ms: Option<f64>,
    /// Node adding the most latency to the synchronous part.
    pub slowest: Option<NodeId>,
    /// Most frequent error messages across the flow's resources (when logs were read).
    #[serde(default)]
    pub top_errors: Vec<FlowError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowError {
    pub node: NodeId,
    pub message: String,
    pub count: f64,
    #[serde(default)]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capacity {
    pub node: NodeId,
    /// Fraction of the limit used at the busiest hour (1.0 = saturated).
    pub utilization: f64,
    /// Traffic multiplier at which the resource saturates (1 / utilization).
    pub headroom: f64,
    /// What was measured, in words: "CPU peaked at 80%".
    pub basis: String,
    /// What the limit is.
    pub limit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub kind: String,
    pub priority: Priority,
    pub title: String,
    pub detail: String,
    /// What to do about it.
    pub action: Option<String>,
    pub nodes: Vec<NodeId>,
    pub flow: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeState {
    Idle,
    Orphaned,
    Zombie,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    pub node: NodeId,
    pub state: LifeState,
    pub reason: String,
    /// List-price estimate in USD for resources with a fixed monthly cost.
    pub monthly_cost: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Insights {
    pub flows: Vec<Flow>,
    pub capacity: Vec<Capacity>,
    pub insights: Vec<Insight>,
    pub lifecycle: Vec<Lifecycle>,
}

pub fn analyze(g: &ArchGraph) -> Insights {
    let flows = flows(g);
    let capacity = capacity(g);
    let lifecycle = lifecycle(g);
    let insights = insights(g, &flows, &capacity);
    Insights { flows, capacity, insights, lifecycle }
}
