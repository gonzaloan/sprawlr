use sprawlr_core::{infer::infer, ArchGraph, Layer};
use clap::{Parser, Subcommand};
use std::collections::BTreeMap;

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
    /// Write the built-in demo account as a snapshot (no AWS needed)
    Demo {
        #[arg(long, default_value = "demo-snapshot.json")]
        out: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Scan { profile, regions, out, no_telemetry } => {
            let opts = sprawlr_scan::ScanOptions { telemetry: !no_telemetry };
            let progress: sprawlr_scan::Progress = std::sync::Arc::new(|m| eprintln!("… {m}"));
            let mut report = sprawlr_scan::scan(profile.as_deref(), &regions, &opts, progress).await?;
            summarize(&mut report.graph);
            for w in &report.warnings {
                eprintln!("warn: {w}");
            }
            if !report.unresolved.is_empty() {
                eprintln!("unresolved references: {}", report.unresolved.len());
            }
            write(&report.graph, &out)
        }
        Cmd::Docs { snapshot, out, json } => {
            let snap: sprawlr_core::Snapshot = serde_json::from_str(&std::fs::read_to_string(&snapshot)?)?;
            let g = ArchGraph::from_snapshot(&snap);
            let date = snap.taken_at.format("%Y-%m-%d").to_string();
            let files = sprawlr_core::docs::generate(&g, &snap.findings, &date);
            for f in &files {
                let path = std::path::Path::new(&out).join(&f.path);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(&path, &f.content)?;
            }
            println!("{} pages written to {out}/", files.len());
            if let Some(j) = json {
                std::fs::write(&j, serde_json::to_string(&files)?)?;
            }
            Ok(())
        }
        Cmd::Demo { out } => {
            let mut g = sprawlr_core::demo::demo_graph();
            summarize(&mut g);
            write(&g, &out)
        }
    }
}

fn summarize(g: &mut ArchGraph) {
    let inf = infer(g);
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
}

fn write(g: &ArchGraph, out: &str) -> anyhow::Result<()> {
    std::fs::write(out, serde_json::to_string_pretty(&g.snapshot())?)?;
    println!("snapshot written to {out}");
    Ok(())
}
