use anyhow::Result;
use clap::Parser;

mod archive;
mod namespace;
mod procdir;

/// Run a command with ephemeral, namespace-private filesystems derived from zip archives.
#[derive(Parser, Debug)]
#[command(name = "fuselage", version, about)]
struct Args {
    /// Extract FILE into a fresh, mutable directory (may be repeated)
    #[arg(long = "dynamic", value_name = "[NAME:]FILE")]
    dynamic: Vec<String>,

    /// Extract FILE into a cached, read-only directory (may be repeated)
    #[arg(long = "static", value_name = "[NAME:]FILE")]
    r#static: Vec<String>,

    /// Find PATH in extracted archives and execute it
    #[arg(long = "run", value_name = "PATH")]
    run: Option<String>,

    /// Command and arguments to run (use after --)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Validate: --run and command are mutually exclusive
    if args.run.is_some() && !args.command.is_empty() {
        anyhow::bail!("--run and a trailing COMMAND are mutually exclusive; use one or the other");
    }

    // --run requires at least one archive
    if args.run.is_some() && args.dynamic.is_empty() && args.r#static.is_empty() {
        anyhow::bail!("--run requires at least one --static or --dynamic archive");
    }

    // Need either --run or a command
    if args.run.is_none() && args.command.is_empty() {
        anyhow::bail!("no command specified; use -- COMMAND or --run PATH");
    }

    todo!("implement fuselage")
}
