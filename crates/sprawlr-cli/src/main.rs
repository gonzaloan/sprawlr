//! Command line driver: parses arguments, picks adapters, calls use cases, prints results.

use clap::{Parser, Subcommand};
use sprawlr_app::{demo::Demo, export_code, ports::Progress, scan_account, ScanOptions, ScanResult};
use sprawlr_aws::Aws;
use sprawlr_domain::{infer::infer, ArchGraph, Layer, Snapshot};
use sprawlr_iac::Scope;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Map an AWS account's architecture (read-only).
#[derive(Parser)]
#[command(name = "sprawlr", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan an AWS account and write a snapshot
    Scan {
        /// AWS profile (defaults to the standard credential chain)
        #[arg(long)]
        profile: Option<String>,
        /// Regions to scan (repeatable)
        #[arg(long = "region", default_value = "us-east-1")]
        regions: Vec<String>,
        #[arg(long, default_value = "snapshot.json")]
        out: String,
        /// Skip CloudWatch alarms/metrics and X-Ray traces
        #[arg(long)]
        no_telemetry: bool,
        /// Also read error lines from CloudWatch Logs (Logs Insights, billed per GB scanned).
        /// Only masked patterns are stored, never raw lines.
        #[arg(long)]
        logs: bool,
    },
    /// Generate Markdown documentation (README.md + apps/<app>.md) from a snapshot
    Docs {
        #[arg(long, default_value = "snapshot.json")]
        snapshot: String,
        /// Output folder
        #[arg(long, default_value = "architecture")]
        out: String,
        /// Also write all pages as one JSON file (used by the web demo)
        #[arg(long)]
        json: Option<String>,
    },
    /// Say what changed between two snapshots of the same account
    Changes {
        #[arg(long)]
        before: String,
        #[arg(long)]
        after: String,
        /// Print the full list as JSON
        #[arg(long)]
        json: bool,
    },
    /// Write Terraform and CloudFormation for the account or one app, from each resource's
    /// configuration as it is now, with the documentation. Values that could be secrets are
    /// never written: they become inputs.
    Export {
        #[arg(long, default_value = "snapshot.json")]
        snapshot: String,
        /// Only this app (defaults to the whole account)
        #[arg(long)]
        app: Option<String>,
        /// Output folder
        #[arg(long, default_value = "export")]
        out: String,
        /// AWS profile (defaults to the standard credential chain)
        #[arg(long)]
        profile: Option<String>,
        /// Read the demo account's configuration instead of AWS
        #[arg(long)]
        demo: bool,
    },
    /// Write the built-in demo account as a snapshot (no AWS needed)
    Demo {
        #[arg(long, default_value = "demo-snapshot.json")]
        out: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let progress: Progress = Arc::new(|m| eprintln!("… {m}"));
    match Cli::parse().cmd {
        Cmd::Scan { profile, regions, out, no_telemetry, logs } => {
            let opts = ScanOptions { health: !no_telemetry, error_logs: logs };
            let r = scan_account(&Aws::new(profile, regions), opts, None, &progress).await?;
            report(&r);
            write(&r.snapshot, &out)
        }
        Cmd::Docs { snapshot, out, json } => {
            let snap = sprawlr_fs::read_snapshot(Path::new(&snapshot))?;
            let files = sprawlr_docs::render(&snap);
            std::fs::create_dir_all(&out)?;
            let n = sprawlr_fs::write_docs(Path::new(&out), &files)?;
            println!("{n} pages written to {out}/");
            if let Some(j) = json {
                std::fs::write(&j, serde_json::to_string(&files)?)?;
            }
            Ok(())
        }
        Cmd::Changes { before, after, json } => {
            let (b, a) = (sprawlr_fs::read_snapshot(Path::new(&before))?, sprawlr_fs::read_snapshot(Path::new(&after))?);
            let Some(c) = sprawlr_domain::changes::compare(&b, &a) else {
                anyhow::bail!("these snapshots are of different accounts");
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                println!("Since {}: {}", c.since.format("%Y-%m-%d %H:%M UTC"), c.summary);
                for r in &c.rows {
                    println!("  {}: {}", r.label, r.text);
                }
            }
            Ok(())
        }
        Cmd::Export { snapshot, app, out, profile, demo } => {
            let snap = sprawlr_fs::read_snapshot(Path::new(&snapshot))?;
            let scope = app.map(Scope::App).unwrap_or(Scope::Account);
            let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let e = if demo {
                export_code(&Demo, &snap, &scope, &date, &progress).await?
            } else {
                export_code(&Aws::for_snapshot(profile, &snap), &snap, &scope, &date, &progress).await?
            };
            let n = sprawlr_fs::write_export(Path::new(&out), &e.files, &sprawlr_docs::render(&snap))?;
            let noun = if e.summary.resources == 1 { "resource" } else { "resources" };
            println!("{} {noun} of {} written to {out}/ ({n} files)", e.summary.resources, e.summary.title);
            for i in &e.summary.inputs {
                println!("  fill in: {} {} ({})", i.resource, i.value, i.why);
            }
            for l in &e.summary.left_out {
                println!("  left out: {}: {}", l.name, l.reason);
            }
            for note in &e.summary.notes {
                println!("  note: {note}");
            }
            println!("Check it: see {out}/README.md");
            Ok(())
        }
        Cmd::Demo { out } => {
            let r = scan_account(&Demo, ScanOptions::default(), None, &progress).await?;
            report(&r);
            // The demo tells what changed since a scan two days earlier.
            write(&sprawlr_app::demo::with_changes(r.snapshot), &out)
        }
    }
}

fn report(r: &ScanResult) {
    let mut g = ArchGraph::from_snapshot(&r.snapshot);
    let inf = infer(&mut g);
    let mut by_layer: BTreeMap<String, usize> = BTreeMap::new();
    for n in g.nodes() {
        *by_layer.entry(format!("{:?}", n.layer())).or_default() += 1;
    }
    let exposed = g.edges().filter(|(f, _, _)| f.layer() == Layer::External).count();
    println!("nodes by layer: {by_layer:?}");
    println!("edges: {} (from internet: {exposed})", g.edges().count());
    println!("apps: {:?}", inf.apps);
    println!("assigned by: {:?}", inf.by_method);
    if !inf.ignored_prefixes.is_empty() {
        println!("ignored org prefixes: {:?}", inf.ignored_prefixes);
    }
    println!("shared between apps: {}  |  unassigned: {}", inf.shared.len(), inf.unassigned.len());
    println!(
        "flows: {}  |  findings: {}  |  to clean up: {}",
        r.snapshot.insights.flows.len(),
        r.snapshot.findings.len(),
        r.snapshot.insights.lifecycle.len()
    );
    for w in &r.warnings {
        eprintln!("warn: {w}");
    }
}

fn write(snapshot: &Snapshot, out: &str) -> anyhow::Result<()> {
    std::fs::write(out, serde_json::to_string_pretty(snapshot)?)?;
    println!("snapshot written to {out}");
    Ok(())
}
