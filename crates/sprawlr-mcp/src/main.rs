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

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let source = match args.iter().position(|a| a == "--snapshot") {
        _ if args.iter().any(|a| a == "--demo") => Source::demo(),
        Some(i) => Source::file(args.get(i + 1).ok_or_else(|| anyhow::anyhow!("--snapshot needs a path"))?.into()),
        None => Source::file(source::default_path().ok_or_else(|| anyhow::anyhow!("no home folder; pass --snapshot <path>"))?),
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("sprawlr-mcp [--snapshot <path> | --demo]\nAn MCP server (stdio) answering questions about the last sprawlr scan.");
        return Ok(());
    }
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
