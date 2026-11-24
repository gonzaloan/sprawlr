//! The account in one line: the level the tray icon shows, the headline under it, and the few
//! items worth opening. Computed with the snapshot so the tray, the popover and the briefing
//! never disagree (ADR 0004).

use crate::analysis::{display_name, env_name};
use crate::insights::{Insights, Priority};
use crate::{ArchGraph, Health, NodeId};
use serde::{Deserialize, Serialize};

/// How many items the tray shows.
pub const TOP: usize = 3;
/// After this long without a read, a level is no longer shown as current.
pub const STALE_AFTER_HOURS: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    ActNow,
    PlanForIt,
    AllClear,
    /// Nothing measured (a scan with telemetry off): "All clear" would be a guess.
    NotMeasured,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::ActNow => "Act now",
            Level::PlanForIt => "Plan for it",
            Level::AllClear => "All clear",
            Level::NotMeasured => "Health not read",
        }
    }
}

/// Where opening an item leads in the full app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Target {
    Flow(String),
    Node(NodeId),
    /// The "Probably not needed" list.
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusItem {
    /// Stable across refreshes: `{kind}:{first node}`.
    pub key: String,
    pub priority: Priority,
    pub title: String,
    pub open: Target,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub act_now: usize,
    pub plan_for_it: usize,
    pub good_to_know: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowErrors {
    pub flow: String,
    pub name: String,
    pub errors: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cleanup {
    pub resources: usize,
    /// USD a month, list price, unrounded.
    pub monthly_cost: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountStatus {
    pub level: Level,
    pub headline: String,
    pub account: Option<String>,
    pub counts: Counts,
    /// Every item in briefing order; the tray shows the first [`TOP`].
    pub items: Vec<StatusItem>,
    pub flows_with_errors: Vec<FlowErrors>,
    pub cleanup: Cleanup,
}

impl AccountStatus {
    pub fn top(&self) -> &[StatusItem] {
        &self.items[..self.items.len().min(TOP)]
    }

    /// One line for a tooltip or a menu: "Act now: 2 to act on now, 3 to plan for".
    pub fn line(&self) -> String {
        format!("{}: {}", self.level.label(), self.headline)
    }
}

/// Something a person would want to hear about between looks at the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub title: String,
    /// Item titles only: notifications can show on a lock screen (no messages, no ARNs).
    pub body: String,
    pub open: Target,
    /// Items behind it, so the caller can avoid repeating itself.
    pub keys: Vec<String>,
}

fn key(kind: &str, first: Option<&NodeId>) -> String {
    format!("{kind}:{}", first.map(|n| n.0.as_str()).unwrap_or(""))
}

/// The status of a graph, with its findings and insights already computed.
pub fn of(g: &ArchGraph, insights: &Insights) -> AccountStatus {
    let mut items: Vec<StatusItem> = insights
        .insights
        .iter()
        .map(|x| StatusItem {
            key: key(&x.kind, x.nodes.first()),
            priority: x.priority,
            title: x.title.clone(),
            open: match (&x.flow, x.nodes.first()) {
                (Some(f), _) => Target::Flow(f.clone()),
                (None, Some(n)) => Target::Node(n.clone()),
                (None, None) => Target::Node(NodeId(String::new())),
            },
        })
        .collect();

    // A firing alarm in production is "act now" even when no flow explains it yet.
    let covered: Vec<&NodeId> = insights.insights.iter().filter(|x| x.priority == Priority::High).flat_map(|x| &x.nodes).collect();
    for n in g.nodes().filter(|n| n.health == Health::Alarm) {
        let prod = n.env.as_ref().is_some_and(|e| env_name(&e.value) == "prod");
        if prod && !covered.contains(&&n.id) {
            items.push(StatusItem {
                key: key("alarm", Some(&n.id)),
                priority: Priority::High,
                title: format!("Alarm firing on {}", display_name(n)),
                open: Target::Node(n.id.clone()),
            });
        }
    }
    items.sort_by_key(|i| i.priority); // stable: briefing order within a priority

    let count = |p: Priority| items.iter().filter(|i| i.priority == p).count();
    let counts = Counts { act_now: count(Priority::High), plan_for_it: count(Priority::Medium), good_to_know: count(Priority::Low) };
    let measured = g
        .nodes()
        .any(|n| n.health != Health::Unknown || n.attrs.get("metrics").is_some_and(|m| m.as_array().is_some_and(|a| !a.is_empty())));
    let (level, headline) = match (counts.act_now, counts.plan_for_it) {
        (h, 0) if h > 0 => (Level::ActNow, format!("{h} to act on now")),
        (h, m) if h > 0 => (Level::ActNow, format!("{h} to act on now, {m} to plan for")),
        (_, m) if m > 0 => (Level::PlanForIt, format!("Nothing urgent, {m} to plan for")),
        _ if !measured => (Level::NotMeasured, "Health not read".to_string()),
        _ => (Level::AllClear, "Nothing needs attention".to_string()),
    };

    let mut flows_with_errors: Vec<FlowErrors> = insights
        .flows
        .iter()
        .filter(|f| f.errors_24h > 0.0)
        .map(|f| FlowErrors { flow: f.id.clone(), name: f.name.clone(), errors: f.errors_24h })
        .collect();
    flows_with_errors.sort_by(|a, b| b.errors.total_cmp(&a.errors).then(a.name.cmp(&b.name)));

    AccountStatus {
        level,
        headline,
        account: g.nodes().find_map(|n| n.account.clone()),
        counts,
        items,
        flows_with_errors,
        cleanup: Cleanup {
            resources: insights.lifecycle.len(),
            monthly_cost: insights.lifecycle.iter().filter_map(|l| l.monthly_cost).sum(),
        },
    }
}

/// What changed for the worse between two reads, worth one notification: the level moving
/// into "act now", or new "act now" items. Items present in both never notify.
pub fn news(before: &AccountStatus, after: &AccountStatus) -> Option<Notice> {
    let urgent = |s: &AccountStatus| -> Vec<StatusItem> { s.items.iter().filter(|i| i.priority == Priority::High).cloned().collect() };
    let was: Vec<String> = urgent(before).into_iter().map(|i| i.key).collect();
    let new: Vec<StatusItem> = urgent(after).into_iter().filter(|i| !was.contains(&i.key)).collect();
    let entered = after.level == Level::ActNow && before.level != Level::ActNow;
    if new.is_empty() && !entered {
        return None;
    }
    let lead = new.first().cloned().or_else(|| urgent(after).into_iter().next())?;
    let more = new.len().saturating_sub(1);
    Some(Notice {
        title: "sprawlr: Act now".into(),
        body: if more > 0 { format!("{} and {more} more", lead.title) } else { lead.title.clone() },
        open: lead.open.clone(),
        keys: if new.is_empty() { vec![lead.key] } else { new.into_iter().map(|i| i.key).collect() },
    })
}

/// Whether a status read at `taken_at` is too old to show as current at `now`.
pub fn is_stale(taken_at: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> bool {
    now - taken_at > chrono::Duration::hours(STALE_AFTER_HOURS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::demo_graph;
    use crate::infer::infer;

    fn demo() -> crate::Snapshot {
        let mut g = demo_graph();
        infer(&mut g);
        g.snapshot()
    }

    fn status_with(items: Vec<(Priority, &str)>, measured: bool) -> AccountStatus {
        let mut s = demo().status.unwrap();
        s.items = items
            .into_iter()
            .map(|(p, k)| StatusItem { key: k.into(), priority: p, title: format!("Item {k}"), open: Target::Node(NodeId(k.into())) })
            .collect();
        s.level = if s.items.iter().any(|i| i.priority == Priority::High) {
            Level::ActNow
        } else if measured {
            Level::AllClear
        } else {
            Level::NotMeasured
        };
        s
    }

    #[test]
    fn demo_reads_act_now_with_the_briefing_items_in_order() {
        let snap = demo();
        let s = snap.status.clone().expect("status computed with the snapshot");
        assert_eq!(s.level, Level::ActNow);
        assert_eq!(s.headline, "2 to act on now, 3 to plan for");
        let top: Vec<&str> = s.top().iter().map(|i| i.title.as_str()).collect();
        assert_eq!(top, ["Errors start at InvoiceProcessor", "checkout-orders-db saturates first", "InvoicesTable is on 2 flows"]);
        let checkout = snap.insights.flows.iter().find(|f| f.name == "Web requests to checkout").unwrap();
        assert_eq!(s.top()[0].open, Target::Flow(checkout.id.clone()), "opens the flow it happens in");
        assert_eq!(s.account.as_deref(), Some("111122223333"));
    }

    #[test]
    fn demo_flows_with_errors_and_cleanup() {
        let s = demo().status.unwrap();
        let flows: Vec<(&str, f64)> = s.flows_with_errors.iter().map(|f| (f.name.as_str(), f.errors)).collect();
        assert_eq!(flows, [("Web requests to checkout", 45.0), ("MonthlyClose runs", 26.0)]);
        assert_eq!(s.cleanup.resources, 5);
        assert!((s.cleanup.monthly_cost - 36.50).abs() < 0.01, "{}", s.cleanup.monthly_cost);
    }

    #[test]
    fn levels_follow_priorities_and_measurement() {
        let mut g = demo_graph();
        infer(&mut g);
        let mut ins = crate::insights::analyze(&g);
        ins.insights.retain(|x| x.priority != Priority::High);
        // Alarms in prod are act-now items of their own; silence them for this case.
        for n in g.nodes_mut() {
            n.health = Health::Ok;
        }
        let s = of(&g, &ins);
        assert_eq!(s.level, Level::PlanForIt);
        assert!(s.headline.starts_with("Nothing urgent, "), "{}", s.headline);

        ins.insights.retain(|x| x.priority == Priority::Low);
        assert_eq!(of(&g, &ins).level, Level::AllClear);
        assert_eq!(of(&g, &ins).headline, "Nothing needs attention");

        for n in g.nodes_mut() {
            n.health = Health::Unknown;
            n.attrs.as_object_mut().map(|a| a.remove("metrics"));
        }
        assert_eq!(of(&g, &ins).level, Level::NotMeasured);
    }

    #[test]
    fn a_prod_alarm_outside_every_insight_is_act_now() {
        let mut g = demo_graph();
        infer(&mut g);
        let ins = crate::insights::analyze(&g);
        let id = g.nodes().find(|n| n.name == "search-indexer").map(|n| n.id.clone()).expect("demo has search-indexer");
        g.node_mut(&id).unwrap().health = Health::Alarm;
        let s = of(&g, &ins);
        assert!(s.items.iter().any(|i| i.key == format!("alarm:{}", id.0) && i.priority == Priority::High), "{:?}", s.items);
        // An alarm already explained by an act-now item adds nothing.
        let s0 = demo().status.unwrap();
        assert!(!s0.items.iter().any(|i| i.key.starts_with("alarm:") && i.title.contains("InvoiceProcessor")), "{:?}", s0.items);
    }

    #[test]
    fn old_snapshots_without_status_still_load() {
        let mut v = serde_json::to_value(demo()).unwrap();
        v.as_object_mut().unwrap().remove("status");
        let s: crate::Snapshot = serde_json::from_value(v).unwrap();
        assert!(s.status.is_none());
    }

    #[test]
    fn news_only_for_what_got_worse() {
        let calm = status_with(vec![(Priority::Medium, "m")], true);
        let one = status_with(vec![(Priority::High, "a"), (Priority::Medium, "m")], true);
        let two = status_with(vec![(Priority::High, "a"), (Priority::High, "b")], true);
        let n = news(&calm, &one).expect("entering act now");
        assert_eq!(n.body, "Item a");
        assert_eq!(n.keys, ["a"]);
        assert!(news(&one, &one).is_none(), "same items, no news");
        let n = news(&one, &two).expect("a new act-now item");
        assert_eq!((n.body.as_str(), n.keys.clone()), ("Item b", vec!["b".to_string()]));
        let both = news(&calm, &two).unwrap();
        assert_eq!(both.body, "Item a and 1 more");
        assert!(news(&two, &calm).is_none(), "getting better never notifies");
    }

    #[test]
    fn staleness() {
        let t = chrono::Utc::now();
        assert!(!is_stale(t, t + chrono::Duration::hours(23)));
        assert!(is_stale(t, t + chrono::Duration::hours(25)));
    }
}
