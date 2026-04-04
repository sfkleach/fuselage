use anyhow::{Context, Result};
use clap::Parser;
use nix::unistd::{Gid, Uid};
use std::ffi::CString;
use std::path::Path;

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

    let ruid = nix::unistd::getuid();
    let rgid = nix::unistd::getgid();
    let euid = nix::unistd::geteuid();

    // Setuid mode: binary is installed setuid-root, caller is an ordinary user.
    // In this mode we have CAP_SYS_ADMIN (euid=0) but we want files created in
    // ~/.fuselage to be owned by the real user, so we temporarily drop euid.
    let is_setuid = euid.is_root() && !ruid.is_root();

    if is_setuid {
        nix::unistd::seteuid(ruid).context("seteuid: failed to drop to real uid")?;
    }

    // Verify/create ~/.fuselage/ before entering the namespace so that
    // ownership checks use the real uid.
    let home = procdir::fuselage_home();
    procdir::setup_home(&home)?;
    procdir::clean_stale_procdirs(&home)?;

    // Create the empty procdir entry on the real filesystem.
    // Inside the namespace this directory will be covered by a tmpfs.
    let pd = procdir::create_procdir(&home)?;

    if is_setuid {
        // Restore euid=0 so we have CAP_SYS_ADMIN for the namespace and mount calls.
        nix::unistd::seteuid(Uid::from_raw(0)).context("seteuid: failed to restore root")?;
    }

    // Enter a private mount namespace (plain for root/setuid, user+mount for unprivileged).
    namespace::enter_namespace()?;

    // Mount a tmpfs over the procdir and create tmp/ inside it.
    procdir::setup_procdir_in_namespace(&pd)?;

    let tmpdir = pd.join("tmp");

    // Process --dynamic archives: parse specs, check for duplicates, extract.
    let dynamic_specs = parse_archive_specs(&args.dynamic)?;
    if !dynamic_specs.is_empty() {
        let dynamic_root = pd.join("dynamic");
        for spec in &dynamic_specs {
            let dest = dynamic_root.join(&spec.name);
            std::fs::create_dir_all(&dest)
                .with_context(|| format!("failed to create {}", dest.display()))?;
            archive::extract_zip(&spec.file, &dest)?;
        }
        // Safety: single-threaded at this point.
        unsafe { std::env::set_var("FUSELAGE_DYNAMIC", &dynamic_root) };
    }

    if is_setuid {
        // All dirs and extracted files were created as root. Recursively chown
        // the entire procdir to the real user so the child can access everything.
        procdir::chown_recursive(&pd, ruid, rgid)
            .context("failed to chown procdir to real user")?;
    }

    // Set FUSELAGE_TMPDIR so the child process can find its scratch space.
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

    // In setuid mode the child drops to the real uid/gid before exec so that
    // sudo and normal uid semantics work inside the command.
    // The parent keeps root so it can umount the tmpfs and rmdir the procdir
    // after the child exits.
    let drop_to = is_setuid.then_some((ruid, rgid));

    run_with_cleanup(&prog, &argv, &pd, drop_to)
}

/// Parse a list of `[NAME:]FILE` specs, returning an error on duplicate names.
fn parse_archive_specs(raw: &[String]) -> Result<Vec<archive::ArchiveSpec>> {
    let mut specs = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for arg in raw {
        let spec = archive::ArchiveSpec::parse(arg)?;
        if seen.contains(&spec.name) {
            anyhow::bail!(
                "duplicate archive name '{}'; use NAME: prefix to disambiguate",
                spec.name
            );
        }
        seen.push(spec.name.clone());
        specs.push(spec);
    }
    Ok(specs)
}

/// Fork, exec `prog` with `argv` in the child, wait for it in the parent,
/// then clean up the procdir before exiting with the child's exit code.
///
/// If `drop_to` is `Some((uid, gid))`, the child permanently drops privileges
/// to that uid/gid before exec (setuid mode). The parent retains its privileges
/// for cleanup.
fn run_with_cleanup(
    prog: &CString,
    argv: &[CString],
    procdir: &Path,
    drop_to: Option<(Uid, Gid)>,
) -> Result<()> {
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::{fork, ForkResult};

    match unsafe { fork() }.context("fork failed")? {
        ForkResult::Child => {
            if let Some((uid, gid)) = drop_to {
                // Drop supplementary groups, then gid, then uid.
                // setresuid/setresgid set real, effective, and saved-set to the same value,
                // making the drop permanent and irreversible.
                if let Err(e) = nix::unistd::setgroups(&[gid]) {
                    eprintln!("fuselage: setgroups failed: {e}");
                    std::process::exit(1);
                }
                if let Err(e) = nix::unistd::setresgid(gid, gid, gid) {
                    eprintln!("fuselage: setresgid failed: {e}");
                    std::process::exit(1);
                }
                if let Err(e) = nix::unistd::setresuid(uid, uid, uid) {
                    eprintln!("fuselage: setresuid failed: {e}");
                    std::process::exit(1);
                }
            }
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
