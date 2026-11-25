//! `sprawlr-mcp`: an MCP server over stdio that answers questions about the last scan.
//!
//! ```text
//! sprawlr-mcp                       # the desktop app's last scan
//! sprawlr-mcp --snapshot scan.json  # a snapshot written by `sprawlr scan`
//! sprawlr-mcp --demo                # the fictional demo account
//! ```
//!
//! Read-only: it never calls AWS. It reads the snapshot again when the file changes.

mod protocol;
mod source;
mod tools;

use source::Source;
use std::io::{BufRead, Write};

const HELP: &str = "sprawlr-mcp [--snapshot <path> | --demo]\n\
An MCP server (stdio) answering questions about the last sprawlr scan. Read-only; never calls AWS.\n\
Without options it reads the desktop app's last scan.";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let source = match args.as_slice() {
        [] => Source::file(source::default_path().ok_or_else(|| anyhow::anyhow!("no home folder; pass --snapshot <path>"))?),
        [flag] if flag == "--demo" => Source::demo(),
        [flag, path] if flag == "--snapshot" => Source::file(path.into()),
        _ => anyhow::bail!("unexpected arguments: {}\n{HELP}", args.join(" ")),
    };
    if let Source::File { path, .. } = &source {
        if !source::exists(path) {
            eprintln!("sprawlr-mcp: no scan at {} yet; tools will say so until there is one", path.display());
        }
    }
    eprintln!("sprawlr-mcp: serving {}", source.describe());

    let mut server = protocol::Server { source };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}
