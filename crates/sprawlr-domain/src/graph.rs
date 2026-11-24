//! The architecture graph: nodes + evidence-merged edges, plus core queries.

use crate::model::*;
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Default)]
pub struct ArchGraph {
    g: StableDiGraph<Node, Edge>,
    index: HashMap<NodeId, NodeIndex>,
}

impl ArchGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a node (re-scans update in place, edges are kept).
    pub fn upsert_node(&mut self, node: Node) -> NodeIndex {
        if let Some(&ix) = self.index.get(&node.id) {
            self.g[ix] = node;
            ix
        } else {
            let id = node.id.clone();
            let ix = self.g.add_node(node);
            self.index.insert(id, ix);
            ix
        }
    }

    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.index.get(id).map(|&ix| &self.g[ix])
    }

    pub fn node_mut(&mut self, id: &NodeId) -> Option<&mut Node> {
        self.index.get(id).copied().map(move |ix| &mut self.g[ix])
    }

    pub fn contains(&self, id: &NodeId) -> bool {
        self.index.contains_key(id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.g.node_weights()
    }

    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.g.node_weights_mut()
    }

    /// Nodes connected to `id` by runtime-dependency edges, in either direction.
    pub fn runtime_neighbors(&self, id: &NodeId) -> Vec<NodeId> {
        let Some(&ix) = self.index.get(id) else { return Vec::new() };
        let mut out: Vec<NodeId> = self
            .g
            .edges_directed(ix, Direction::Outgoing)
            .filter(|e| e.weight().kind.is_runtime_dependency())
            .map(|e| self.g[e.target()].id.clone())
            .chain(
                self.g
                    .edges_directed(ix, Direction::Incoming)
                    .filter(|e| e.weight().kind.is_runtime_dependency())
                    .map(|e| self.g[e.source()].id.clone()),
            )
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Add evidence for `from --kind--> to`. Edges are unique per (from, to, kind);
    /// new evidence is merged into the existing edge.
    /// Returns false if either endpoint is unknown.
    pub fn add_evidence(&mut self, from: &NodeId, to: &NodeId, kind: EdgeKind, ev: Evidence) -> bool {
        let (Some(&a), Some(&b)) = (self.index.get(from), self.index.get(to)) else {
            return false;
        };
        let existing = self.g.edges_connecting(a, b).find(|e| e.weight().kind == kind).map(|e| e.id());
        match existing {
            Some(eid) => {
                let edge = &mut self.g[eid];
                if !edge.evidence.iter().any(|x| x.kind == ev.kind && x.source == ev.source) {
                    edge.evidence.push(ev);
                }
            }
            None => {
                self.g.add_edge(a, b, Edge { kind, evidence: vec![ev] });
            }
        }
        true
    }

    pub fn edges(&self) -> impl Iterator<Item = (&Node, &Node, &Edge)> {
        self.g.edge_references().map(|e| (&self.g[e.source()], &self.g[e.target()], e.weight()))
    }

    /// Direct children in the containment tree (e.g. subnets of a VPC).
    pub fn children(&self, parent: &NodeId) -> Vec<&Node> {
        self.nodes().filter(|n| n.parent.as_ref() == Some(parent)).collect()
    }

    /// Everything that breaks if `id` fails: transitive dependents over runtime edges.
    pub fn blast_radius(&self, id: &NodeId) -> BTreeSet<NodeId> {
        let mut out = BTreeSet::new();
        let Some(&start) = self.index.get(id) else { return out };
        let mut queue = VecDeque::from([start]);
        while let Some(ix) = queue.pop_front() {
            for e in self.g.edges_directed(ix, Direction::Incoming) {
                if !e.weight().kind.is_runtime_dependency() {
                    continue;
                }
                let dep = &self.g[e.source()];
                if out.insert(dep.id.clone()) {
                    queue.push_back(e.source());
                }
            }
        }
        out
    }

    /// Nodes grouped by inferred app (unassigned under `None`).
    pub fn by_app(&self) -> HashMap<Option<String>, Vec<&Node>> {
        let mut m: HashMap<Option<String>, Vec<&Node>> = HashMap::new();
        for n in self.nodes() {
            m.entry(n.app.as_ref().map(|a| a.value.clone())).or_default().push(n);
        }
        m
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut nodes: Vec<Node> = self.nodes().cloned().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        let mut edges: Vec<SnapshotEdge> =
            self.edges().map(|(f, t, e)| SnapshotEdge { from: f.id.clone(), to: t.id.clone(), edge: e.clone() }).collect();
        edges.sort_by(|a, b| (&a.from, &a.to, a.edge.kind).cmp(&(&b.from, &b.to, b.edge.kind)));
        let insights = crate::insights::analyze(self);
        let status = Some(crate::status::of(self, &insights));
        Snapshot { taken_at: chrono::Utc::now(), findings: crate::analysis::findings(self), insights, status, nodes, edges }
    }

    /// Rebuild a graph from a stored snapshot (e.g. a file opened in the app).
    pub fn from_snapshot(s: &Snapshot) -> Self {
        let mut g = ArchGraph::new();
        for n in &s.nodes {
            g.upsert_node(n.clone());
        }
        for e in &s.edges {
            for ev in &e.edge.evidence {
                g.add_evidence(&e.from, &e.to, e.edge.kind, ev.clone());
            }
        }
        g
    }
}

/// Serializable, deterministic view of the graph — stored locally and diffed over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub taken_at: chrono::DateTime<chrono::Utc>,
    pub nodes: Vec<Node>,
    pub edges: Vec<SnapshotEdge>,
    /// Computed when the snapshot is taken, so every consumer shows the same findings.
    #[serde(default)]
    pub findings: Vec<crate::analysis::Finding>,
    /// Flows, capacity, risks and lifecycle, computed with the findings.
    #[serde(default)]
    pub insights: crate::insights::Insights,
    /// The account in one line, for the tray and the briefing. `None` in snapshots written
    /// before it existed: use `status_or_compute`.
    #[serde(default)]
    pub status: Option<crate::status::AccountStatus>,
}

impl Snapshot {
    /// The stored status, or one computed from the snapshot (older files have none).
    pub fn status_or_compute(&self) -> crate::status::AccountStatus {
        match &self.status {
            Some(s) => s.clone(),
            None => crate::status::of(&ArchGraph::from_snapshot(self), &self.insights),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEdge {
    pub from: NodeId,
    pub to: NodeId,
    #[serde(flatten)]
    pub edge: Edge,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn ev(kind: EvidenceKind, source: &str) -> Evidence {
        Evidence { kind, source: source.into(), detail: None, seen_at: Utc::now() }
    }

    /// ALB -> ECS -> RDS, ECS uses a secret, Lambda reads RDS.
    fn sample() -> ArchGraph {
        let mut g = ArchGraph::new();
        g.upsert_node(Node::new("vpc-1", NodeKind::Vpc, "main"));
        let mut subnet = Node::new("subnet-a", NodeKind::Subnet, "private-a");
        subnet.parent = Some("vpc-1".into());
        g.upsert_node(subnet);
        g.upsert_node(Node::new("alb", NodeKind::LoadBalancer, "checkout-alb"));
        g.upsert_node(Node::new("ecs", NodeKind::EcsService, "checkout-api"));
        g.upsert_node(Node::new("rds", NodeKind::RdsCluster, "orders-db"));
        g.upsert_node(Node::new("sec", NodeKind::Secret, "orders-db-creds"));
        g.upsert_node(Node::new("fn", NodeKind::LambdaFunction, "report-job"));

        g.add_evidence(&"alb".into(), &"ecs".into(), EdgeKind::RoutesTo, ev(EvidenceKind::Declared, "elb:target_group"));
        g.add_evidence(&"ecs".into(), &"rds".into(), EdgeKind::Reads, ev(EvidenceKind::Declared, "env:DB_HOST"));
        g.add_evidence(&"ecs".into(), &"sec".into(), EdgeKind::UsesSecret, ev(EvidenceKind::Declared, "ecs:task_def.secrets"));
        g.add_evidence(&"fn".into(), &"rds".into(), EdgeKind::Reads, ev(EvidenceKind::Permitted, "iam:role/report-job"));
        g
    }

    #[test]
    fn evidence_is_merged_per_edge() {
        let mut g = sample();
        g.add_evidence(&"ecs".into(), &"rds".into(), EdgeKind::Reads, ev(EvidenceKind::Observed, "vpc_flow_logs"));
        g.add_evidence(&"ecs".into(), &"rds".into(), EdgeKind::Reads, ev(EvidenceKind::Observed, "vpc_flow_logs"));
        let reads: Vec<_> = g.edges().filter(|(f, t, _)| f.id.0 == "ecs" && t.id.0 == "rds").collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].2.evidence.len(), 2);
        assert_eq!(reads[0].2.strength(), Some(EvidenceKind::Observed));
    }

    #[test]
    fn blast_radius_follows_dependents() {
        let g = sample();
        let br = g.blast_radius(&"rds".into());
        let ids: Vec<_> = br.iter().map(|n| n.0.as_str()).collect();
        assert_eq!(ids, vec!["alb", "ecs", "fn"]);
    }

    #[test]
    fn permission_only_edges_are_flagged() {
        let g = sample();
        let flagged: Vec<_> = g.edges().filter(|(_, _, e)| e.is_permission_only()).collect();
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].0.id.0, "fn");
    }

    #[test]
    fn containment_is_separate_from_edges() {
        let g = sample();
        assert_eq!(g.children(&"vpc-1".into()).len(), 1);
        assert_eq!(g.edges().count(), 4);
    }

    #[test]
    fn snapshot_roundtrips() {
        let snap = sample().snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), 7);
        assert_eq!(back.edges.len(), 4);
        let again = ArchGraph::from_snapshot(&back);
        assert_eq!(again.edges().count(), 4);
        assert_eq!(again.blast_radius(&"rds".into()).len(), 3);
    }
}
