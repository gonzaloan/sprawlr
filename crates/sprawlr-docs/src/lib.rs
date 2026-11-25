//! Markdown documentation generated from the graph: one index for the account and one page per app,
//! with Mermaid diagrams so the result renders on GitHub, GitLab, Confluence and most wikis.

use serde::{Deserialize, Serialize};
use sprawlr_domain::analysis::{display_name, env_differences, env_name, kind_label, verb, Finding, Severity};
use sprawlr_domain::{ArchGraph, Edge, EdgeKind, EvidenceKind, Health, InferenceMethod, Layer, Node, NodeId, NodeKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocFile {
    /// Relative path, e.g. `README.md`, `apps/checkout.md`.
    pub path: String,
    pub title: String,
    pub content: String,
}

/// All documentation for the account: `README.md` plus `apps/<app>.md`.
/// Pages for a snapshot, dated by when it was taken, with what changed since the previous
/// scan when the snapshot knows.
pub fn render(snapshot: &sprawlr_domain::Snapshot) -> Vec<DocFile> {
    let g = ArchGraph::from_snapshot(snapshot);
    let mut files = generate(&g, &snapshot.findings, &snapshot.taken_at.format("%Y-%m-%d").to_string());
    if let (Some(c), Some(index)) = (&snapshot.changes, files.iter_mut().find(|f| f.path == "README.md")) {
        let section = changes_section(c);
        match index.content.find("\n## ") {
            Some(at) => index.content.insert_str(at + 1, &section),
            None => index.content.push_str(&section),
        }
    }
    files
}

/// "Changes since 2026-11-22": what moved between two scans, in lists a reviewer can scan.
fn changes_section(c: &sprawlr_domain::changes::Changes) -> String {
    let mut s = format!("## Changes since {}\n\n{}.\n\n", c.since.format("%Y-%m-%d %H:%M UTC"), c.headline());
    let mut list = |title: &str, items: Vec<String>| {
        if !items.is_empty() {
            let _ = writeln!(s, "**{title}**\n");
            for i in items {
                let _ = writeln!(s, "- {i}");
            }
            s.push('\n');
        }
    };
    list("New", c.added.iter().map(|i| format!("{} ({})", i.name, i.kind)).collect());
    list("Removed", c.removed.iter().map(|i| format!("{} ({})", i.name, i.kind)).collect());
    list("Settings", c.settings.iter().map(|x| format!("{}: {} {} → {}", x.resource.name, x.label, x.before, x.after)).collect());
    list(
        "Connections",
        c.connections_added
            .iter()
            .map(|x| format!("new: {} {} {}", x.from, x.verb, x.to))
            .chain(c.connections_removed.iter().map(|x| format!("gone: {} {} {}", x.from, x.verb, x.to)))
            .collect(),
    );
    list("Needs attention now", c.attention_new.clone());
    list("No longer needs attention", c.attention_resolved.clone());
    list("New findings", c.findings_new.clone());
    list("Resolved findings", c.findings_resolved.clone());
    let health = |h: &sprawlr_domain::Health| serde_json::to_value(h).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    list("Health", c.health.iter().map(|h| format!("{}: {} → {}", h.resource.name, health(&h.before), health(&h.after))).collect());
    list("New error messages", c.new_errors.iter().map(|e| format!("`{}`", e.replace('`', "'"))).collect());
    s
}

pub fn generate(g: &ArchGraph, findings: &[Finding], generated_on: &str) -> Vec<DocFile> {
    let ctx = Ctx::new(g, findings, generated_on);
    let mut out = vec![DocFile { path: "README.md".into(), title: "Whole account".into(), content: ctx.index() }];
    for app in ctx.apps() {
        out.push(DocFile { path: format!("apps/{}.md", slug(&app)), title: app.clone(), content: ctx.app_page(&app) });
    }
    out
}

struct Ctx<'a> {
    g: &'a ArchGraph,
    findings: &'a [Finding],
    date: &'a str,
    account: String,
    by_node: HashMap<&'a NodeId, Vec<&'a Finding>>,
}

const LAYER_ORDER: [Layer; 6] = [Layer::External, Layer::Edge, Layer::Compute, Layer::Integration, Layer::Data, Layer::Identity];

fn layer_rank(n: &Node) -> usize {
    LAYER_ORDER.iter().position(|l| *l == n.layer()).unwrap_or(9)
}

fn mappable(n: &Node) -> bool {
    !matches!(n.layer(), Layer::Scope | Layer::Network)
}

fn env_of(n: &Node) -> Option<String> {
    n.env.as_ref().map(|e| env_name(&e.value))
}

fn app_of(n: &Node) -> Option<&str> {
    n.app.as_ref().map(|a| a.value.as_str())
}

fn method_text(m: InferenceMethod) -> &'static str {
    match m {
        InferenceMethod::Tag => "tag",
        InferenceMethod::Stack => "CloudFormation stack",
        InferenceMethod::Account => "account",
        InferenceMethod::Naming => "name",
        InferenceMethod::Containment => "location",
        InferenceMethod::Graph => "connections",
        InferenceMethod::Llm => "AI suggestion",
        InferenceMethod::UserOverride => "set manually",
    }
}

fn evidence_text(e: &Edge) -> String {
    let mut by_kind: BTreeMap<std::cmp::Reverse<EvidenceKind>, Vec<&str>> = BTreeMap::new();
    for ev in &e.evidence {
        by_kind.entry(std::cmp::Reverse(ev.kind)).or_default().push(ev.source.as_str());
    }
    by_kind
        .into_iter()
        .map(|(std::cmp::Reverse(k), src)| {
            let label = match k {
                EvidenceKind::Observed => "seen in traffic",
                EvidenceKind::Declared => "configured",
                EvidenceKind::Permitted => "allowed by IAM",
            };
            format!("{label} (`{}`)", src.join("`, `"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn health_text(h: Health) -> &'static str {
    match h {
        Health::Ok => "Healthy",
        Health::Warning => "Warning",
        Health::Alarm => "**Alarm**",
        Health::Unknown => "",
    }
}

fn sev(s: Severity) -> &'static str {
    match s {
        Severity::High => "High",
        Severity::Medium => "Medium",
        Severity::Low => "Low",
    }
}

/// File-name-safe app name: app names can come from tags or edited snapshots.
pub fn slug(s: &str) -> String {
    let out: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' }).collect();
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "app".into()
    } else {
        out
    }
}

/// Mermaid-safe label text.
fn esc(s: &str) -> String {
    s.replace('"', "#quot;").replace('<', "#lt;").replace('>', "#gt;").replace('|', "#124;")
}

/// Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

impl<'a> Ctx<'a> {
    fn new(g: &'a ArchGraph, findings: &'a [Finding], date: &'a str) -> Self {
        let account = g.nodes().find_map(|n| n.account.clone()).unwrap_or_else(|| "unknown".into());
        let mut by_node: HashMap<&NodeId, Vec<&Finding>> = HashMap::new();
        for f in findings {
            by_node.entry(&f.node).or_default().push(f);
        }
        Ctx { g, findings, date, account, by_node }
    }

    fn apps(&self) -> Vec<String> {
        let set: BTreeSet<String> = self.g.nodes().filter_map(|n| app_of(n).map(String::from)).collect();
        set.into_iter().collect()
    }

    fn sorted<'b>(&self, mut v: Vec<&'b Node>) -> Vec<&'b Node> {
        v.sort_by(|a, b| layer_rank(a).cmp(&layer_rank(b)).then(display_name(a).cmp(&display_name(b))));
        v
    }

    fn vpc_of(&self, n: &Node) -> Option<&'a Node> {
        let mut cur = n.parent.as_ref().and_then(|p| self.g.node(p));
        for _ in 0..8 {
            let c = cur?;
            if c.kind == NodeKind::Vpc {
                return Some(c);
            }
            cur = c.parent.as_ref().and_then(|p| self.g.node(p));
        }
        None
    }

    fn runs_in(&self, n: &Node) -> String {
        self.vpc_of(n).map(|v| v.name.clone()).unwrap_or_else(|| "AWS-managed".into())
    }

    fn header(&self, title: &str) -> String {
        format!(
            "# {title}\n\n_Generated by sprawlr from a read-only scan of account {} on {}. \
             Regenerate instead of editing by hand; anything inferred says how it was inferred._\n\n",
            self.account, self.date
        )
    }

    // ------------------------------------------------------------------ index

    fn index(&self) -> String {
        let mut s = self.header(&format!("Architecture of account {}", self.account));
        let apps = self.apps();

        let ins = sprawlr_domain::insights::analyze(self.g);
        s.push_str(&self.attention(&ins));

        let _ = writeln!(s, "## Apps\n\n| App | Resources | Environments | To review |\n|---|---|---|---|");
        for app in &apps {
            let members: Vec<&Node> = self.g.nodes().filter(|n| app_of(n) == Some(app)).collect();
            let envs: BTreeSet<String> = members.iter().filter_map(|n| env_of(n)).collect();
            let fs: Vec<&&Finding> = members.iter().flat_map(|n| self.by_node.get(&n.id).into_iter().flatten()).collect();
            let high = fs.iter().filter(|f| f.severity == Severity::High).count();
            let review = match (fs.len(), high) {
                (0, _) => "Nothing".to_string(),
                (n, 0) => n.to_string(),
                (n, h) => format!("{n} ({h} high)"),
            };
            let _ = writeln!(
                s,
                "| [{app}](apps/{}.md) | {} | {} | {review} |",
                slug(app),
                members.len(),
                envs.into_iter().collect::<Vec<_>>().join(", ")
            );
        }

        let _ = writeln!(s, "\n## How apps connect\n\n```mermaid\n{}```\n", self.overview_mermaid());
        s.push_str(&self.flows_section(&ins));
        s.push_str(&self.capacity_section(&ins));
        s.push_str(&self.cleanup_section(&ins));

        let (shared, unassigned) = self.orphans();
        if !shared.is_empty() {
            let _ = writeln!(s, "## Shared by several apps\n");
            for (n, users) in &shared {
                let _ = writeln!(s, "- **{}** ({}), used by {}", display_name(n), kind_label(&n.kind), users.join(", "));
            }
            s.push('\n');
        }
        if !unassigned.is_empty() {
            let _ = writeln!(s, "## Not linked to an app\n\nTag these with `app=<name>` to place them.\n");
            for n in &unassigned {
                let _ = writeln!(s, "- {} ({})", display_name(n), kind_label(&n.kind));
            }
            s.push('\n');
        }
        s.push_str(&self.findings_table(self.findings.iter().collect(), true));
        s
    }

    fn name(&self, id: &NodeId) -> String {
        self.g.node(id).map(display_name).unwrap_or_else(|| id.0.clone())
    }

    fn attention(&self, ins: &sprawlr_domain::insights::Insights) -> String {
        use sprawlr_domain::insights::Priority;
        if ins.insights.is_empty() {
            return String::new();
        }
        let mut s = String::from("## What needs attention\n\n");
        for x in &ins.insights {
            let tag = match x.priority {
                Priority::High => "Act now",
                Priority::Medium => "Plan for it",
                Priority::Low => "Good to know",
            };
            let _ = write!(s, "- **{tag}: {}.** {}", x.title, x.detail);
            if let Some(a) = &x.action {
                let _ = write!(s, " *{a}*");
            }
            s.push('\n');
        }
        s.push('\n');
        s
    }

    fn flows_section(&self, ins: &sprawlr_domain::insights::Insights) -> String {
        use sprawlr_domain::insights::{Mode, Trigger};
        if ins.flows.is_empty() {
            return String::new();
        }
        let mut s = String::from("## Flows\n\nHow requests and events move through the account, found from entry points (public endpoints, schedules). Solid arrows wait for an answer; open arrows happen later, through a queue or event.\n\n");
        for f in &ins.flows {
            let runs = match f.runs_24h {
                Some(r) if (r - 1.0).abs() < 0.05 => "1 run a day".to_string(),
                Some(r) => format!("{} runs a day", fmt_num(r)),
                None => "runs not measured".into(),
            };
            // Only worth saying when it is close enough to matter.
            let weakest = f
                .nodes
                .iter()
                .filter_map(|id| ins.capacity.iter().find(|c| &c.node == id && c.headroom < 10.0))
                .min_by(|a, b| a.headroom.total_cmp(&b.headroom));
            let _ = writeln!(
                s,
                "### {}\n\n{}, {runs}, {} errors in 24 h{}.\n",
                f.name,
                match f.trigger {
                    Trigger::Internet => "Starts from the internet",
                    Trigger::Event => "Starts when data arrives",
                    Trigger::SelfStarted => "Starts on its own",
                },
                fmt_num(f.errors_24h),
                weakest
                    .map(|c| format!(", breaks first at {} ({})", sprawlr_domain::insights::fmt_x(c.headroom), self.name(&c.node)))
                    .unwrap_or_default()
            );
            let _ = writeln!(s, "```mermaid\nsequenceDiagram");
            let mut ids: Vec<&NodeId> = Vec::new();
            let pid = |ids: &Vec<&NodeId>, id: &NodeId| ids.iter().position(|x| *x == id).map(|i| format!("p{i}"));
            if f.trigger == Trigger::Internet {
                let _ = writeln!(s, "  participant internet as Internet");
            }
            for id in &f.nodes {
                ids.push(id);
                let label = self.name(id).replace([';', '#', ':'], " ");
                let _ = writeln!(s, "  participant {} as {}", pid(&ids, id).unwrap(), label);
            }
            if f.trigger == Trigger::Internet {
                let _ = writeln!(s, "  internet->>{}: request", pid(&ids, &f.entry).unwrap());
            }
            for h in &f.hops {
                let (Some(a), Some(b)) = (pid(&ids, &h.from), pid(&ids, &h.to)) else { continue };
                let arrow = if h.mode == Mode::Async { "-)" } else { "->>" };
                let iam = if h.inferred { " (IAM allows)" } else { "" };
                let v = if h.mode == Mode::Async && matches!(h.kind, EdgeKind::Reads | EdgeKind::Subscribes) {
                    "delivers to"
                } else {
                    verb(h.kind)
                };
                let _ = writeln!(s, "  {a}{arrow}{b}: {v}{iam}");
            }
            let _ = writeln!(s, "```\n");
            if !f.top_errors.is_empty() {
                let _ = writeln!(s, "Most frequent errors in its logs (24 h):\n");
                for e in &f.top_errors {
                    let _ = writeln!(s, "- {} × {}: `{}`", fmt_num(e.count), self.name(&e.node), e.message.replace('`', "'"));
                }
                s.push('\n');
            }
        }
        s
    }

    fn capacity_section(&self, ins: &sprawlr_domain::insights::Insights) -> String {
        let rows: Vec<_> = ins.capacity.iter().filter(|c| c.headroom.is_finite() && c.headroom < 20.0).take(8).collect();
        if rows.is_empty() {
            return String::new();
        }
        let mut s = String::from("## If traffic grows\n\nEach resource's busiest hour against its limit, and the traffic multiplier at which it saturates (assuming everything grows one to one).\n\n| Resource | Busiest hour | Limit | Saturates at |\n|---|---|---|---|\n");
        for c in rows {
            let _ = writeln!(
                s,
                "| {} | {} | {} | **{}** |",
                cell(&self.name(&c.node)),
                cell(&c.basis),
                cell(&c.limit),
                sprawlr_domain::insights::fmt_x(c.headroom)
            );
        }
        s.push('\n');
        s
    }

    fn cleanup_section(&self, ins: &sprawlr_domain::insights::Insights) -> String {
        use sprawlr_domain::insights::LifeState;
        if ins.lifecycle.is_empty() {
            return String::new();
        }
        let total: f64 = ins.lifecycle.iter().filter_map(|l| l.monthly_cost).sum();
        let mut s = format!(
            "## Probably not needed\n\n{} resources look unused{}. Check the evidence before removing anything.\n\n| Resource | State | Why | Per month |\n|---|---|---|---|\n",
            ins.lifecycle.len(),
            if total > 0.0 { format!(", about ${total:.2} a month at list price") } else { String::new() }
        );
        for l in &ins.lifecycle {
            let state = match l.state {
                LifeState::Zombie => "Costs money, unused",
                LifeState::Idle => "Idle",
                LifeState::Orphaned => "Orphaned",
            };
            let cost = l.monthly_cost.map(|c| format!("${c:.2}")).unwrap_or_else(|| "usage-based".into());
            let _ = writeln!(s, "| {} | {state} | {} | {cost} |", cell(&self.name(&l.node)), cell(&l.reason));
        }
        s.push('\n');
        s
    }

    fn orphans(&self) -> (Vec<(&'a Node, Vec<String>)>, Vec<&'a Node>) {
        let mut shared = Vec::new();
        let mut unassigned = Vec::new();
        for n in self.g.nodes() {
            if n.app.is_some() || !mappable(n) || n.layer() == Layer::External {
                continue;
            }
            let users: BTreeSet<String> =
                self.g.runtime_neighbors(&n.id).iter().filter_map(|id| self.g.node(id).and_then(app_of).map(String::from)).collect();
            if users.len() > 1 {
                shared.push((n, users.into_iter().collect()));
            } else {
                unassigned.push(n);
            }
        }
        (shared, unassigned)
    }

    fn overview_mermaid(&self) -> String {
        let mut s = String::from("flowchart LR\n");
        let key = |n: &Node| -> Option<String> {
            if let Some(a) = app_of(n) {
                return Some(format!("app_{}", a.replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
            }
            if n.kind == NodeKind::Internet {
                return Some("internet".into());
            }
            if n.kind == NodeKind::ExternalEndpoint {
                return Some(format!("ext_{}", n.name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
            }
            None
        };
        let (shared, _) = self.orphans();
        let shared_ids: HashMap<&NodeId, String> = shared.iter().enumerate().map(|(i, (n, _))| (&n.id, format!("shared{i}"))).collect();
        let key_of = |n: &Node| key(n).or_else(|| shared_ids.get(&n.id).cloned());

        let mut declared: BTreeSet<String> = BTreeSet::new();
        for app in self.apps() {
            let id = key_of(self.g.nodes().find(|n| app_of(n) == Some(&app)).unwrap()).unwrap();
            let _ = writeln!(s, "  {id}[\"{}\"]", esc(&app));
            declared.insert(id);
        }
        for (n, _) in &shared {
            let _ = writeln!(s, "  {}[(\"{}\")]", shared_ids[&n.id], esc(&display_name(n)));
        }
        let mut edges: BTreeMap<(String, String), EvidenceKind> = BTreeMap::new();
        for (f, t, e) in self.g.edges() {
            if !e.kind.is_runtime_dependency() && e.kind != EdgeKind::RoutesTo {
                continue;
            }
            let (Some(a), Some(b)) = (key_of(f), key_of(t)) else { continue };
            if a == b {
                continue;
            }
            let st = e.strength().unwrap_or(EvidenceKind::Permitted);
            let slot = edges.entry((a, b)).or_insert(st);
            *slot = (*slot).max(st);
        }
        if edges.keys().any(|(a, _)| a == "internet") {
            let _ = writeln!(s, "  internet((\"Internet\"))");
        }
        for n in self.g.nodes().filter(|n| n.kind == NodeKind::ExternalEndpoint) {
            let id = key_of(n).unwrap();
            if edges.keys().any(|(a, b)| *a == id || *b == id) {
                let _ = writeln!(s, "  {id}{{{{\"{}\"}}}}", esc(&n.name));
            }
        }
        for ((a, b), st) in edges {
            let _ = writeln!(s, "  {a} {} {b}", arrow(st));
        }
        s
    }

    // ------------------------------------------------------------------ app page

    fn app_page(&self, app: &str) -> String {
        let g = self.g;
        let members: Vec<&Node> = self.sorted(g.nodes().filter(|n| app_of(n) == Some(app) && mappable(n)).collect());
        let ids: BTreeSet<&NodeId> = members.iter().map(|n| &n.id).collect();
        let envs: BTreeSet<String> = members.iter().filter_map(|n| env_of(n)).collect();
        let edges: Vec<(&Node, &Node, &Edge)> =
            g.edges().filter(|(f, t, e)| e.kind != EdgeKind::NetworkReachable && (ids.contains(&f.id) || ids.contains(&t.id))).collect();
        let app_findings: Vec<&Finding> = self.findings.iter().filter(|f| ids.contains(&f.node)).collect();

        let mut s = self.header(app);

        // --- summary
        let _ = writeln!(s, "## Summary\n");
        let per_env: Vec<String> =
            envs.iter().map(|e| format!("{} in {e}", members.iter().filter(|n| env_of(n).as_deref() == Some(e)).count())).collect();
        let _ = writeln!(
            s,
            "- **{} resources**{}",
            members.len(),
            if per_env.is_empty() { String::new() } else { format!(": {}", per_env.join(", ")) }
        );
        let entries: Vec<String> = edges
            .iter()
            .filter(|(f, t, _)| f.kind == NodeKind::Internet && ids.contains(&t.id))
            .map(|(_, t, _)| format!("{} ({})", display_name(t), kind_label(&t.kind)))
            .collect();
        let _ =
            writeln!(s, "- **Entry points from the internet:** {}", if entries.is_empty() { "none".into() } else { entries.join(", ") });
        let vpcs: BTreeSet<String> = members.iter().map(|n| self.runs_in(n)).collect();
        let _ = writeln!(s, "- **Runs in:** {}", vpcs.into_iter().collect::<Vec<_>>().join(", "));
        let (uses, used_by) = self.cross_app(app, &edges, &ids);
        if !uses.is_empty() {
            let _ = writeln!(s, "- **Depends on:** {}", uses.join(", "));
        }
        if !used_by.is_empty() {
            let _ = writeln!(s, "- **Used by:** {}", used_by.join(", "));
        }
        let high = app_findings.iter().filter(|f| f.severity == Severity::High).count();
        let _ = writeln!(
            s,
            "- **To review:** {}",
            match (app_findings.len(), high) {
                (0, _) => "nothing".to_string(),
                (1, 0) => "1 finding".to_string(),
                (n, 0) => format!("{n} findings"),
                (n, h) => format!("{n} findings, {h} high"),
            }
        );

        // --- diagrams, one per environment
        let mut ordered: Vec<String> = envs.iter().cloned().collect();
        ordered.sort_by_key(|e| (["prod", "staging", "test", "dev"].iter().position(|x| x == e).unwrap_or(9), e.clone()));
        let env_list: Vec<Option<String>> = if ordered.is_empty() { vec![None] } else { ordered.into_iter().map(Some).collect() };
        for env in &env_list {
            let title = env.as_ref().map(|e| format!("Architecture in {e}")).unwrap_or_else(|| "Architecture".into());
            let _ = writeln!(s, "\n## {title}\n\n```mermaid\n{}```", self.app_mermaid(&members, env.as_deref()));
        }
        let _ = writeln!(
            s,
            "\nThick arrows were seen in traffic, plain arrows are configured, dotted arrows are only allowed by IAM. \
             Rounded boxes belong to other apps or are shared."
        );

        // --- components
        let _ = writeln!(
            s,
            "\n## Components\n\n| Resource | Type | Environment | Runs in | Health | Placed in this app by |\n|---|---|---|---|---|---|"
        );
        for n in &members {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {} |",
                cell(&display_name(n)),
                kind_label(&n.kind),
                env_of(n).unwrap_or_default(),
                cell(&self.runs_in(n)),
                health_text(n.health),
                n.app.as_ref().map(|a| method_text(a.method)).unwrap_or_default()
            );
        }

        // --- health (telemetry)
        let mut health_rows = Vec::new();
        for n in &members {
            for m in n.attrs.get("metrics").and_then(|v| v.as_array()).into_iter().flatten() {
                let label = m.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let unit = m.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                let value = m.get("summary").and_then(|v| v.as_f64());
                let when = m.get("summary_label").and_then(|v| v.as_str()).unwrap_or("");
                let shown = match (value, unit) {
                    (None, _) => "no data".to_string(),
                    (Some(v), "count") => format!("{} ({when})", fmt_num(v)),
                    (Some(v), "bytes") => format!("{} ({when})", fmt_bytes(v)),
                    (Some(v), "ms") if v >= 1000.0 => format!("{:.1} s ({when})", v / 1000.0),
                    (Some(v), u) => format!("{} {u} ({when})", fmt_num(v)),
                };
                let flag = m.get("alert").and_then(|v| v.as_str()).map(|a| format!("**{a}**")).unwrap_or_default();
                health_rows.push(format!("| {} | {label} | {shown} | {flag} |", cell(&display_name(n))));
            }
            for a in n.attrs.get("alarms").and_then(|v| v.as_array()).into_iter().flatten() {
                if a.get("state").and_then(|v| v.as_str()) == Some("ALARM") {
                    let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("alarm");
                    health_rows.push(format!("| {} | Alarm `{}` | in ALARM | **firing** |", cell(&display_name(n)), cell(name)));
                }
            }
        }
        // --- error messages from logs
        let mut log_rows = Vec::new();
        for n in &members {
            if let Some(l) = sprawlr_domain::logs::of(n) {
                for p in &l.patterns {
                    log_rows.push(format!(
                        "| {} | `{}` | {} | {} |",
                        cell(&display_name(n)),
                        cell(&p.message.replace('`', "'")),
                        fmt_num(p.count),
                        p.last_seen.as_deref().map(|t| t.get(..16).unwrap_or(t)).unwrap_or("")
                    ));
                }
            }
        }

        if !health_rows.is_empty() {
            let _ = writeln!(s, "\n## Health in the last 24 hours\n\n| Resource | Signal | Value | |\n|---|---|---|---|");
            for r in health_rows {
                let _ = writeln!(s, "{r}");
            }
        }

        if !log_rows.is_empty() {
            let _ = writeln!(s, "\n## Errors in logs, last 24 hours\n\nMessages are reduced to patterns: numbers, ids, addresses and values are masked, and the raw lines are never stored.\n\n| Resource | Message | Times | Last seen (UTC) |\n|---|---|---|---|");
            for r in log_rows {
                let _ = writeln!(s, "{r}");
            }
        }

        // --- communication
        let _ = writeln!(s, "\n## How components communicate\n\n| From | | To | How we know |\n|---|---|---|---|");
        let mut flows = edges.clone();
        flows.sort_by(|a, b| {
            (layer_rank(a.0), display_name(a.0), display_name(a.1)).cmp(&(layer_rank(b.0), display_name(b.0), display_name(b.1)))
        });
        for (f, t, e) in &flows {
            let _ =
                writeln!(s, "| {} | {} | {} | {} |", cell(&self.label(f, app)), verb(e.kind), cell(&self.label(t, app)), evidence_text(e));
        }

        // --- network
        s.push_str(&self.network_section(&members, &edges, &ids));

        // --- secrets & identity
        s.push_str(&self.secrets_section(&members, &edges, &ids));

        // --- environments
        if envs.len() > 1 {
            let _ = writeln!(s, "\n## Differences between environments\n");
            let diffs = env_differences(g, app);
            if diffs.is_empty() {
                let _ = writeln!(s, "The environments have the same resources and settings.");
            }
            for d in diffs {
                let _ = writeln!(s, "- {d}");
            }
        }

        // --- blast radius
        let mut radius: Vec<(&Node, Vec<String>)> = members
            .iter()
            .filter(|n| matches!(n.layer(), Layer::Data | Layer::Integration | Layer::Identity | Layer::Compute))
            .map(|n| {
                let mut hit: Vec<String> = g
                    .blast_radius(&n.id)
                    .iter()
                    .filter_map(|id| g.node(id))
                    .filter(|m| m.kind != NodeKind::Internet)
                    .map(|m| self.label(m, app))
                    .collect();
                hit.sort();
                (*n, hit)
            })
            .filter(|(_, hit)| !hit.is_empty())
            .collect();
        radius.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(display_name(a.0).cmp(&display_name(b.0))));
        if !radius.is_empty() {
            let _ = writeln!(s, "\n## What breaks if something fails\n");
            for (n, hit) in radius.iter().take(6) {
                let _ = writeln!(s, "- **{}** ({}): {}", display_name(n), env_of(n).unwrap_or_default(), hit.join(", "));
            }
        }

        s.push('\n');
        s.push_str(&self.findings_table(app_findings, false));
        s
    }

    /// Name, plus the owning app when it is not the page's app.
    fn label(&self, n: &Node, app: &str) -> String {
        match app_of(n) {
            Some(a) if a != app => format!("{} ({a})", display_name(n)),
            None if n.kind == NodeKind::ExternalEndpoint => format!("{} (third party)", display_name(n)),
            None if mappable(n) && n.kind != NodeKind::Internet => format!("{} (shared)", display_name(n)),
            _ => display_name(n),
        }
    }

    fn cross_app(&self, app: &str, edges: &[(&Node, &Node, &Edge)], ids: &BTreeSet<&NodeId>) -> (Vec<String>, Vec<String>) {
        let mut uses = BTreeSet::new();
        let mut used_by = BTreeSet::new();
        for (f, t, e) in edges {
            if !e.kind.is_runtime_dependency() {
                continue;
            }
            if ids.contains(&f.id) && !ids.contains(&t.id) && t.kind != NodeKind::Internet {
                uses.insert(app_of(t).filter(|a| *a != app).map(String::from).unwrap_or_else(|| self.label(t, app)));
            }
            if ids.contains(&t.id) && !ids.contains(&f.id) && f.kind != NodeKind::Internet {
                used_by.insert(app_of(f).filter(|a| *a != app).map(String::from).unwrap_or_else(|| self.label(f, app)));
            }
        }
        (uses.into_iter().collect(), used_by.into_iter().collect())
    }

    fn app_mermaid(&self, members: &[&Node], env: Option<&str>) -> String {
        let in_env = |n: &Node| env.is_none() || env_of(n).as_deref().is_none_or(|e| Some(e) == env);
        let core: Vec<&Node> = members.iter().copied().filter(|n| in_env(n)).collect();
        let core_ids: BTreeSet<&NodeId> = core.iter().map(|n| &n.id).collect();

        let mut shown: Vec<&Node> = core.clone();
        let mut lines: Vec<(NodeId, NodeId, EvidenceKind, EdgeKind)> = Vec::new();
        for (f, t, e) in self.g.edges() {
            if e.kind == EdgeKind::NetworkReachable || (!core_ids.contains(&f.id) && !core_ids.contains(&t.id)) {
                continue;
            }
            for end in [f, t] {
                if !shown.iter().any(|n| n.id == end.id) && (mappable(end) || end.kind == NodeKind::Internet) {
                    shown.push(end);
                }
            }
            if shown.iter().any(|n| n.id == f.id) && shown.iter().any(|n| n.id == t.id) {
                lines.push((f.id.clone(), t.id.clone(), e.strength().unwrap_or(EvidenceKind::Permitted), e.kind));
            }
        }
        let ids: HashMap<&NodeId, String> = shown.iter().enumerate().map(|(i, n)| (&n.id, format!("n{i}"))).collect();

        let mut s = String::from("flowchart LR\n");
        let node_line = |n: &Node| -> String {
            let id = &ids[&n.id];
            let text = format!("{}<br/><small>{}</small>", esc(&display_name(n)), esc(&kind_label(&n.kind)));
            if n.kind == NodeKind::Internet {
                format!("{id}((\"Internet\"))")
            } else if core_ids.contains(&n.id) {
                format!("{id}[\"{text}\"]")
            } else {
                let owner = app_of(n).map(|a| format!("<br/><small>in {}</small>", esc(a))).unwrap_or_default();
                format!("{id}(\"{}{owner}\")", esc(&display_name(n)))
            }
        };
        // Group by VPC territory.
        let mut by_vpc: BTreeMap<Option<String>, Vec<&Node>> = BTreeMap::new();
        for n in &shown {
            by_vpc.entry(self.vpc_of(n).map(|v| v.id.0.clone())).or_default().push(n);
        }
        for (i, (vpc, nodes)) in by_vpc.iter().enumerate() {
            match vpc.as_ref().and_then(|v| self.g.node(&NodeId(v.clone()))) {
                Some(v) => {
                    let cidr = v.attrs.get("cidr").and_then(|c| c.as_str()).map(|c| format!(" ({c})")).unwrap_or_default();
                    let _ = writeln!(s, "  subgraph vpc{i}[\"{}{}\"]", esc(&v.name), esc(&cidr));
                    for n in nodes {
                        let _ = writeln!(s, "    {}", node_line(n));
                    }
                    let _ = writeln!(s, "  end");
                }
                None => {
                    for n in nodes {
                        let _ = writeln!(s, "  {}", node_line(n));
                    }
                }
            }
        }
        for (f, t, st, kind) in &lines {
            let _ = writeln!(s, "  {} {}|{}| {}", ids[f], arrow_open(*st), verb(*kind), ids[t]);
        }
        s
    }

    fn network_section(&self, members: &[&Node], edges: &[(&Node, &Node, &Edge)], ids: &BTreeSet<&NodeId>) -> String {
        let mut s = String::from("\n## Network\n\n");
        let mut by_vpc: BTreeMap<String, Vec<&Node>> = BTreeMap::new();
        for n in members {
            by_vpc.entry(self.runs_in(n)).or_default().push(n);
        }
        for (vpc, nodes) in &by_vpc {
            let names: Vec<String> = nodes.iter().map(|n| display_name(n)).collect();
            if vpc == "AWS-managed" {
                let _ = writeln!(s, "- **Outside any VPC** (AWS-managed endpoints): {}", names.join(", "));
                continue;
            }
            let cidr = self
                .g
                .nodes()
                .find(|v| v.kind == NodeKind::Vpc && &v.name == vpc)
                .and_then(|v| v.attrs.get("cidr").and_then(|c| c.as_str()).map(String::from));
            let _ = writeln!(s, "- **{vpc}**{}: {}", cidr.map(|c| format!(" ({c})")).unwrap_or_default(), names.join(", "));
            for n in nodes {
                let subnets = n
                    .attrs
                    .get("subnets")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let sgs: Vec<String> = n
                    .attrs
                    .get("security_groups")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .map(|id| self.g.node(&NodeId(id.into())).map(|g| g.name.clone()).unwrap_or_else(|| id.into()))
                            .collect()
                    })
                    .unwrap_or_default();
                if subnets.is_empty() && sgs.is_empty() {
                    continue;
                }
                let mut parts = Vec::new();
                if !subnets.is_empty() {
                    parts.push(format!("subnets {}", subnets.join(", ")));
                }
                if !sgs.is_empty() {
                    parts.push(format!("security groups {}", sgs.join(", ")));
                }
                let _ = writeln!(s, "  - {}: {}", display_name(n), parts.join("; "));
            }
        }
        let exposed: Vec<String> = edges
            .iter()
            .filter(|(f, t, _)| f.kind == NodeKind::Internet && ids.contains(&t.id))
            .map(|(_, t, e)| format!("{} ({})", display_name(t), verb(e.kind)))
            .collect();
        let open_sgs: BTreeSet<String> = members
            .iter()
            .flat_map(|n| n.attrs.get("security_groups").and_then(|v| v.as_array()).cloned().unwrap_or_default())
            .filter_map(|id| self.g.node(&NodeId(id.as_str()?.to_string())))
            .filter(|sg| sg.attrs.get("open_to_internet") == Some(&serde_json::Value::Bool(true)))
            .map(|sg| {
                let rules: Vec<&str> = sg
                    .attrs
                    .get("ingress")
                    .and_then(|i| i.as_array())
                    .map(|a| a.iter().filter_map(|r| r.as_str()).collect())
                    .unwrap_or_default();
                if rules.is_empty() {
                    sg.name.clone()
                } else {
                    format!("{} ({})", sg.name, rules.join(", "))
                }
            })
            .collect();
        if !exposed.is_empty() || !open_sgs.is_empty() {
            let _ = writeln!(
                s,
                "\n**Internet exposure.** Entry points: {}.",
                if exposed.is_empty() { "none".into() } else { exposed.join(", ") }
            );
            if !open_sgs.is_empty() {
                let _ = writeln!(s, "Security groups open to 0.0.0.0/0: {}.", open_sgs.into_iter().collect::<Vec<_>>().join(", "));
            }
        }
        s
    }

    fn secrets_section(&self, members: &[&Node], edges: &[(&Node, &Node, &Edge)], ids: &BTreeSet<&NodeId>) -> String {
        let mut s = String::from("\n## Secrets, keys and identities\n\n");
        let mut any = false;
        let mut users: BTreeMap<&NodeId, Vec<String>> = BTreeMap::new();
        for (f, t, e) in edges {
            if matches!(e.kind, EdgeKind::UsesSecret | EdgeKind::EncryptedWith | EdgeKind::AssumesRole) && ids.contains(&f.id) {
                users.entry(&t.id).or_default().push(display_name(f));
            }
        }
        for (id, who) in &users {
            let Some(t) = self.g.node(id) else { continue };
            let mut extra = Vec::new();
            match t.attrs.get("rotation_enabled").and_then(|v| v.as_bool()) {
                Some(true) => extra.push("rotation on".to_string()),
                Some(false) => extra.push("**rotation off**".to_string()),
                None => {}
            }
            if let Some(d) = t.attrs.get("last_rotated_days").and_then(|v| v.as_i64()) {
                extra.push(format!("rotated {d} days ago"));
            }
            let extra = if extra.is_empty() { String::new() } else { format!(" ({})", extra.join(", ")) };
            let _ = writeln!(s, "- **{}**, {}{extra}: used by {}", display_name(t), kind_label(&t.kind), who.join(", "));
            any = true;
        }
        for n in members {
            if let Some(keys) = n.attrs.get("env_plaintext_secret_keys").and_then(|v| v.as_array()).filter(|a| !a.is_empty()) {
                let keys: Vec<&str> = keys.iter().filter_map(|k| k.as_str()).collect();
                let _ = writeln!(s, "- **{}** keeps `{}` as plain text in environment variables.", display_name(n), keys.join("`, `"));
                any = true;
            }
        }
        if !any {
            let _ = writeln!(s, "No secrets, keys or roles detected for this app.");
        }
        let _ = writeln!(s, "\nSecret values are never read; only names, usage and rotation settings.");
        s
    }

    fn findings_table(&self, fs: Vec<&Finding>, with_app: bool) -> String {
        if fs.is_empty() {
            return "## To review\n\nNothing flagged.\n".into();
        }
        let mut s = String::from("## To review\n\n");
        let _ = writeln!(
            s,
            "| Severity | Resource |{} Finding |\n|---|---|{}---|",
            if with_app { " App |" } else { "" },
            if with_app { "---|" } else { "" }
        );
        for f in fs {
            let n = self.g.node(&f.node);
            let name = n.map(display_name).unwrap_or_else(|| f.node.0.clone());
            let app = if with_app { format!(" {} |", n.and_then(app_of).unwrap_or("")) } else { String::new() };
            let _ = writeln!(s, "| {} | {} |{app} **{}**. {} |", sev(f.severity), cell(&name), cell(&f.title), cell(&f.detail));
        }
        s
    }
}

fn fmt_bytes(v: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut x = v;
    let mut i = 0;
    while x >= 1000.0 && i < units.len() - 1 {
        x /= 1000.0;
        i += 1;
    }
    if x >= 10.0 || i == 0 {
        format!("{x:.0} {}", units[i])
    } else {
        format!("{x:.1} {}", units[i])
    }
}

fn fmt_num(v: f64) -> String {
    match v.abs() {
        x if x >= 1_000_000.0 => format!("{:.1}M", v / 1_000_000.0),
        x if x >= 10_000.0 => format!("{:.0}k", v / 1000.0),
        x if x >= 1000.0 => format!("{:.1}k", v / 1000.0),
        _ if (v - v.round()).abs() < 0.05 => format!("{v:.0}"),
        _ => format!("{v:.1}"),
    }
}

fn arrow(st: EvidenceKind) -> &'static str {
    match st {
        EvidenceKind::Observed => "==>",
        EvidenceKind::Declared => "-->",
        EvidenceKind::Permitted => "-.->",
    }
}

/// Arrow form that accepts a `|label|` right after it.
fn arrow_open(st: EvidenceKind) -> &'static str {
    arrow(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprawlr_domain::analysis::findings;
    use sprawlr_domain::fixtures::demo_graph;
    use sprawlr_domain::infer::infer;

    fn docs() -> Vec<DocFile> {
        let mut g = demo_graph();
        infer(&mut g);
        let f = findings(&g);
        generate(&g, &f, "2026-11-24")
    }

    #[test]
    fn one_page_per_app_plus_index() {
        let paths: Vec<String> = docs().into_iter().map(|d| d.path).collect();
        assert_eq!(paths, vec!["README.md", "apps/billing.md", "apps/checkout.md", "apps/reporting.md", "apps/search.md"]);
    }

    #[test]
    fn checkout_page_has_every_section() {
        let d = docs().into_iter().find(|d| d.path == "apps/checkout.md").unwrap().content;
        for h in [
            "## Summary",
            "## Architecture in prod",
            "## Architecture in dev",
            "## Components",
            "## How components communicate",
            "## Network",
            "## Secrets, keys and identities",
            "## Differences between environments",
            "## What breaks if something fails",
            "## To review",
        ] {
            assert!(d.contains(h), "missing {h}\n{d}");
        }
        assert!(d.contains("subgraph"), "VPCs become subgraphs");
        assert!(d.contains("==>|writes|"), "observed traffic uses thick arrows");
        assert!(d.contains("multi_az"));
        assert!(!d.contains("sk_live"), "no secret values");
    }

    #[test]
    fn hostile_app_names_cannot_escape_the_folder() {
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("Payments API"), "payments-api");
    }

    #[test]
    fn index_links_apps_and_lists_shared() {
        let d = docs().into_iter().next().unwrap().content;
        assert!(d.contains("[checkout](apps/checkout.md)"));
        assert!(d.contains("## Shared by several apps"));
        assert!(d.contains("platform-events"));
        assert!(d.contains("## What needs attention"));
        assert!(d.contains("sequenceDiagram"), "flows as sequence diagrams");
        assert!(d.contains("-)"), "async hops use open arrows");
        assert!(d.contains("## If traffic grows") && d.contains("checkout-orders-db"));
        assert!(d.contains("## Probably not needed") && d.contains("nat-legacy-dev"));
    }
}
