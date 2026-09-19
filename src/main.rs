use std::io::Read;
use std::path::PathBuf;

use clap::Parser;
use pgexplain::model::parse_explain_json;
use pgexplain::render::render;

#[derive(Parser)]
#[command(
    name = "pgexplain",
    about = "Renders Postgres EXPLAIN (ANALYZE, FORMAT JSON) as a readable tree, highlighting the actually-slow node"
)]
struct Cli {
    /// A file containing `EXPLAIN (ANALYZE, FORMAT JSON) ...` output.
    /// Omit to read from stdin, e.g.
    /// `psql -Atc "EXPLAIN (ANALYZE, FORMAT JSON) SELECT ..." | pgexplain`.
    file: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let json = match cli.file {
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let plan = parse_explain_json(&json).map_err(|e| anyhow::anyhow!(e))?;
    print!("{}", render(&plan));
    Ok(())
}
