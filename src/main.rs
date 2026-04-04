use anyhow::Result;
use clap::Parser;
use std::ffi::CString;

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

    // Verify/create ~/.fuselage/ before entering the namespace so that
    // ownership checks use the real uid.
    let home = procdir::fuselage_home();
    procdir::setup_home(&home)?;
    procdir::clean_stale_procdirs(&home)?;

    // Create the empty procdir entry on the real filesystem.
    // Inside the namespace this directory will be covered by a tmpfs.
    let pd = procdir::create_procdir(&home)?;

    // Enter a private mount namespace (user namespace for unprivileged callers).
    namespace::enter_namespace()?;

    // Mount a tmpfs over the procdir and create tmp/ inside it.
    procdir::setup_procdir_in_namespace(&pd)?;

    // Set FUSELAGE_TMPDIR so the child process can find its scratch space.
    let tmpdir = pd.join("tmp");
    // Safety: single-threaded at this point (we haven't forked yet).
    unsafe { std::env::set_var("FUSELAGE_TMPDIR", &tmpdir) };

    // Build the argv for exec.
    let cmd = &args.command[0];
    let cmd_args = &args.command[1..];

    let prog = CString::new(cmd.as_str())
        .with_context(|| format!("command contains a null byte: {cmd:?}"))?;
    let mut argv: Vec<CString> = Vec::with_capacity(1 + cmd_args.len());
    argv.push(prog.clone());
    for arg in cmd_args {
        argv.push(
            CString::new(arg.as_str())
                .with_context(|| format!("argument contains a null byte: {arg:?}"))?,
        );
    }

    run_with_cleanup(&prog, &argv, &pd)
}

/// Fork, exec `prog` with `argv` in the child, wait for it in the parent,
/// then clean up the procdir before exiting with the child's exit code.
fn run_with_cleanup(prog: &CString, argv: &[CString], procdir: &std::path::Path) -> Result<()> {
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::{fork, ForkResult};

    match unsafe { fork() }.context("fork failed")? {
        ForkResult::Child => {
            // execvp searches $PATH when `prog` contains no slash, matching `env` semantics.
            let err = nix::unistd::execvp(prog, argv).unwrap_err();
            eprintln!("fuselage: exec {:?}: {}", prog, err);
            std::process::exit(127);
        }
        ForkResult::Parent { child } => {
            let status = waitpid(child, None).context("waitpid failed")?;
            procdir::cleanup_procdir(procdir);
            match status {
                WaitStatus::Exited(_, code) => std::process::exit(code),
                WaitStatus::Signaled(_, sig, _) => {
                    // Re-raise so the parent exits with the same signal,
                    // giving the caller an accurate exit status.
                    let _ = nix::sys::signal::raise(sig);
                    std::process::exit(128 + sig as i32);
                }
                _ => std::process::exit(1),
            }
        }
    }
}

use anyhow::Context;
