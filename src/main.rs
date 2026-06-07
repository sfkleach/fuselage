use anyhow::{Context, Result};
use clap::Parser;
use nix::unistd::{Gid, Uid};
use std::ffi::CString;
use std::path::{Path, PathBuf};

mod archive;
mod b64stream;
mod namespace;
mod procdir;

/// Run a command with ephemeral, namespace-private filesystems derived from zip archives.
#[derive(Parser, Debug)]
#[command(name = "fuselage", version, about)]
struct Args {
    /// Extract FILE into a fresh, mutable directory (may be repeated)
    #[arg(short = 'd', long = "dynamic", value_name = "[NAME:]FILE")]
    dynamic: Vec<String>,

    /// Create an empty writable directory at NAME or /run/fuselage/NAME (may be repeated)
    #[arg(long = "dynamic-empty", value_name = "NAME")]
    dynamic_empty: Vec<String>,

    /// Extract FILE into a cached, read-only directory (may be repeated)
    #[arg(short = 's', long = "static", value_name = "[NAME:]FILE")]
    r#static: Vec<String>,

    /// Cache zip --static archives as squashfs images (keyed by SHA-256 content hash).
    /// Disabled by default so that confidential archives leave no traces on disk.
    #[arg(long = "cache-static")]
    cache_static: bool,

    /// Find PATH in extracted archives and execute it
    #[arg(long = "run", value_name = "PATH")]
    run: Option<String>,

    /// Command and arguments to run (use after --)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // --run requires at least one archive
    if args.run.is_some()
        && args.dynamic.is_empty()
        && args.dynamic_empty.is_empty()
        && args.r#static.is_empty()
    {
        anyhow::bail!("--run requires at least one --static or --dynamic archive");
    }

    // Need either --run or a command
    if args.run.is_none() && args.command.is_empty() {
        anyhow::bail!("no command specified; use -- COMMAND or --run PATH");
    }

    let ruid = nix::unistd::getuid();
    let rgid = nix::unistd::getgid();
    let euid = nix::unistd::geteuid();

    // Whether we have real CAP_SYS_ADMIN (setuid-root or running as actual root).
    // Only in this mode can we use loop devices for squashfs mounting.
    let is_privileged = euid.is_root();

    // Setuid mode: binary is installed setuid-root, caller is an ordinary user.
    let is_setuid = is_privileged && !ruid.is_root();

    // Parse all archive specs up front so errors are caught before any filesystem work.
    let mut seen_names: Vec<String> = Vec::new();
    let dynamic_specs = parse_archive_specs(&args.dynamic, &mut seen_names)?;
    let empty_specs = parse_empty_specs(&args.dynamic_empty, &mut seen_names)?;
    let static_specs = parse_archive_specs(&args.r#static, &mut seen_names)?;

    // Reject fixed-path mounts in unprivileged mode at parse time, before any work.
    if !is_privileged {
        for spec in dynamic_specs.iter().chain(static_specs.iter()) {
            if matches!(spec.mount, archive::MountName::Fixed(_)) {
                anyhow::bail!(
                    "fixed mount path {:?} requires privileged (setuid-root) mode",
                    spec.mount.as_str()
                );
            }
        }
        for spec in &empty_specs {
            if matches!(spec.mount, archive::MountName::Fixed(_)) {
                anyhow::bail!(
                    "fixed mount path {:?} requires privileged (setuid-root) mode",
                    spec.mount.as_str()
                );
            }
        }
    }

    if is_setuid {
        nix::unistd::seteuid(ruid).context("seteuid: failed to drop to real uid")?;
    }

    // Verify/create ~/.fuselage/ before entering the namespace so that
    // ownership checks use the real uid.
    let home = procdir::fuselage_home();
    procdir::setup_home(&home)?;
    procdir::clean_stale_procdirs(&home)?;

    // Create the empty procdir entry on the real filesystem.
    let pd = procdir::create_procdir(&home)?;

    if is_setuid {
        nix::unistd::seteuid(Uid::from_raw(0)).context("seteuid: failed to restore root")?;
    }

    // While privileged, create /run/fuselage/ and any fixed-path subdirectories.
    // This must happen before dropping privileges and before entering the namespace.
    if is_privileged {
        let fixed_paths = fixed_mount_paths(&dynamic_specs, &empty_specs, &static_specs);
        if !fixed_paths.is_empty() {
            std::fs::create_dir_all("/run/fuselage").context("failed to create /run/fuselage")?;
            for path in &fixed_paths {
                std::fs::create_dir_all(path)
                    .with_context(|| format!("failed to create {}", path.display()))?;
            }
        }
    }

    // Enter a private mount namespace.
    namespace::enter_namespace()?;

    // Mount a tmpfs over the procdir and create tmp/ inside it.
    procdir::setup_procdir_in_namespace(&pd)?;

    let tmpdir = pd.join("tmp");

    // ── Dynamic archives ──────────────────────────────────────────────────────
    // Zip/squashfs: extract to dest (procdir or fixed path).
    // Empty: just mkdir the dest.
    let dynamic_root = pd.join("dynamic");
    let has_dynamic = !dynamic_specs.is_empty() || !empty_specs.is_empty();

    if has_dynamic {
        std::fs::create_dir_all(&dynamic_root)
            .with_context(|| format!("failed to create {}", dynamic_root.display()))?;

        for spec in &dynamic_specs {
            let dest = mount_dest(&spec.mount, &dynamic_root);
            std::fs::create_dir_all(&dest)
                .with_context(|| format!("failed to create {}", dest.display()))?;
            let (archive_file, fmt) = resolve_archive(spec, &pd.join("decode"))?;
            match fmt {
                archive::ArchiveFormat::Zip => archive::extract_zip(&archive_file, &dest)?,
                archive::ArchiveFormat::Squashfs => {
                    archive::extract_squashfs(&archive_file, &dest, 0)?
                }
                archive::ArchiveFormat::ElfSquashfs(offset) => {
                    archive::extract_squashfs(&archive_file, &dest, offset)?
                }
            }
        }

        for spec in &empty_specs {
            let dest = mount_dest(&spec.mount, &dynamic_root);
            std::fs::create_dir_all(&dest)
                .with_context(|| format!("failed to create {}", dest.display()))?;
        }

        // Safety: single-threaded at this point.
        unsafe { std::env::set_var("FUSELAGE_DYNAMIC", &dynamic_root) };
    }

    // ── Static archives ───────────────────────────────────────────────────────
    // Phase 1: prepare content (extraction into procdir or cache lookup).
    //          Deferred mounts collected so chown can run before any mounts.
    // Phase 2: chown everything in pd (setuid mode only).
    // Phase 3: apply mounts (loop or bind-ro).

    let static_root = pd.join("static");
    let cache_dir = procdir::cache_dir(&home);

    // Collected mount actions — executed after chown.
    enum MountAction {
        LoopSfs(PathBuf, u64),          // loop-mount .sfs onto dest with byte offset
        ExtractSfsBindRo(PathBuf, u64), // extract .sfs to dest (with offset), then bind-ro
        BindRoSelf,                     // dest already has content; bind-ro it
        BindRoFrom(PathBuf),            // bind-mount from external dir to dest
    }
    let mut mount_actions: Vec<(PathBuf, MountAction)> = Vec::new();

    if !static_specs.is_empty() {
        std::fs::create_dir_all(&static_root)
            .with_context(|| format!("failed to create {}", static_root.display()))?;
    }

    for spec in &static_specs {
        let dest = mount_dest(&spec.mount, &static_root);
        std::fs::create_dir_all(&dest)
            .with_context(|| format!("failed to create {}", dest.display()))?;

        let (archive_file, fmt) = resolve_archive(spec, &pd.join("decode"))?;
        let action = match fmt {
            // ── .sfs input ───────────────────────────────────────────────────
            // Use directly — no caching needed, the file is already optimal.
            archive::ArchiveFormat::Squashfs => {
                if is_privileged {
                    MountAction::LoopSfs(archive_file, 0)
                } else {
                    MountAction::ExtractSfsBindRo(archive_file, 0)
                }
            }

            // ── ELF+squashfs input ────────────────────────────────────────
            archive::ArchiveFormat::ElfSquashfs(offset) => {
                if is_privileged {
                    MountAction::LoopSfs(archive_file, offset)
                } else {
                    MountAction::ExtractSfsBindRo(archive_file, offset)
                }
            }

            // ── zip input, no caching ─────────────────────────────────────
            archive::ArchiveFormat::Zip if !args.cache_static => {
                archive::extract_zip(&archive_file, &dest)?;
                MountAction::BindRoSelf
            }

            // ── zip input, caching enabled ────────────────────────────────
            archive::ArchiveFormat::Zip => {
                std::fs::create_dir_all(&cache_dir)?;
                let hash = archive::compute_sha256(&archive_file)?;
                let sfs_path = cache_dir.join(format!("{hash}.sfs"));
                let dir_path = cache_dir.join(&hash);
                let sentinel = cache_dir.join(format!("{hash}.complete"));

                if !sentinel.exists() {
                    // Cache miss — build the cache entry.
                    let tmp = pd.join(format!(".tmp-{}", spec.mount.as_str()));
                    std::fs::create_dir_all(&tmp)?;

                    let built_sfs = archive::zip_to_squashfs(&archive_file, &sfs_path, &tmp)?;

                    if !built_sfs {
                        // mksquashfs not available — fall back to directory cache.
                        archive::extract_zip(&archive_file, &dir_path)?;
                    }

                    std::fs::remove_dir_all(&tmp).ok();
                    std::fs::File::create(&sentinel).context("failed to write cache sentinel")?;
                } else {
                    // Cache hit — refresh sentinel mtime to record last use.
                    procdir::touch_sentinel(&sentinel)?;
                }

                if sfs_path.exists() {
                    if is_privileged {
                        MountAction::LoopSfs(sfs_path, 0)
                    } else {
                        MountAction::ExtractSfsBindRo(sfs_path, 0)
                    }
                } else {
                    // Directory cache (mksquashfs fallback).
                    MountAction::BindRoFrom(dir_path)
                }
            }
        };

        mount_actions.push((dest, action));
    }

    if !static_specs.is_empty() {
        // Safety: single-threaded at this point.
        unsafe { std::env::set_var("FUSELAGE_STATIC", &static_root) };
    }

    // Phase 2: in setuid mode, chown everything before locking any mounts.
    if is_setuid {
        procdir::chown_recursive(&pd, ruid, rgid)
            .context("failed to chown procdir to real user")?;
    }

    // Phase 3: apply deferred mounts.
    for (dest, action) in mount_actions {
        match action {
            MountAction::LoopSfs(sfs, offset) => {
                procdir::loop_mount_sfs(&sfs, &dest, offset)?;
            }
            MountAction::ExtractSfsBindRo(sfs, offset) => {
                archive::extract_squashfs(&sfs, &dest, offset)?;
                procdir::bind_mount_readonly(&dest)?;
            }
            MountAction::BindRoSelf => {
                procdir::bind_mount_readonly(&dest)?;
            }
            MountAction::BindRoFrom(src) => {
                procdir::bind_mount_readonly_from(&src, &dest)?;
            }
        }
    }

    // Set FUSELAGE_TMPDIR so the child process can find its scratch space.
    // Safety: single-threaded at this point (we haven't forked yet).
    unsafe { std::env::set_var("FUSELAGE_TMPDIR", &tmpdir) };

    // Build the argv for exec.
    let (exec_path, extra_args): (String, &[String]) = if let Some(ref run_path) = args.run {
        let resolved = resolve_run_path(
            run_path,
            &dynamic_specs,
            &empty_specs,
            &static_specs,
            &dynamic_root,
            &static_root,
        )?;
        (resolved, &args.command)
    } else {
        (args.command[0].clone(), &args.command[1..])
    };

    let prog = CString::new(exec_path.as_str())
        .with_context(|| format!("command contains a null byte: {exec_path:?}"))?;
    let mut argv: Vec<CString> = Vec::with_capacity(1 + extra_args.len());
    argv.push(prog.clone());
    for arg in extra_args {
        argv.push(
            CString::new(arg.as_str())
                .with_context(|| format!("argument contains a null byte: {arg:?}"))?,
        );
    }

    // In setuid mode the child drops to the real uid/gid before exec.
    // The parent keeps root so it can umount the tmpfs and rmdir the procdir.
    let drop_to = is_setuid.then_some((ruid, rgid));

    let fixed_paths = fixed_mount_paths(&dynamic_specs, &empty_specs, &static_specs);
    run_with_cleanup(&prog, &argv, &pd, drop_to, &cache_dir, fixed_paths)
}

/// Resolve the destination path for a mount, using the fixed path directly
/// or constructing a path under `root` for relative names.
fn mount_dest(mount: &archive::MountName, root: &Path) -> PathBuf {
    match mount {
        archive::MountName::Fixed(p) => p.clone(),
        archive::MountName::Relative(name) => root.join(name),
    }
}

/// Collect all fixed `/run/fuselage/NAME` paths from all spec lists.
fn fixed_mount_paths(
    dynamic_specs: &[archive::ArchiveSpec],
    empty_specs: &[archive::EmptySpec],
    static_specs: &[archive::ArchiveSpec],
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for spec in dynamic_specs.iter().chain(static_specs.iter()) {
        if let archive::MountName::Fixed(p) = &spec.mount {
            paths.push(p.clone());
        }
    }
    for spec in empty_specs {
        if let archive::MountName::Fixed(p) = &spec.mount {
            paths.push(p.clone());
        }
    }
    paths
}

/// Resolve an archive spec to a `(file_path, format)` pair.
///
/// Tries direct format detection first. If the file is neither zip nor squashfs,
/// attempts to decode it as a base64-encoded archive (with optional leading `#`
/// comment lines) into `decode_dir`. The decoded file is written to
/// `decode_dir/<name>.decoded` within the private tmpfs so it never touches
/// persistent storage.
fn resolve_archive(
    spec: &archive::ArchiveSpec,
    decode_dir: &Path,
) -> Result<(PathBuf, archive::ArchiveFormat)> {
    if let Ok(fmt) = archive::detect_format(&spec.file) {
        return Ok((spec.file.clone(), fmt));
    }

    // Unrecognised binary format — try base64 with optional '#' comment lines.
    std::fs::create_dir_all(decode_dir)
        .with_context(|| format!("failed to create decode dir {}", decode_dir.display()))?;
    let decoded = decode_dir.join(format!("{}.decoded", spec.mount.as_str()));
    let is_b64 = archive::try_decode_base64(&spec.file, &decoded)?;

    if !is_b64 {
        anyhow::bail!(
            "{}: unrecognised archive format (not zip, squashfs, or base64)",
            spec.file.display()
        );
    }

    let fmt = archive::detect_format(&decoded).with_context(|| {
        format!(
            "{}: base64 decoded to an unrecognised archive format",
            spec.file.display()
        )
    })?;

    Ok((decoded, fmt))
}

/// Parse a list of `[NAME:]FILE` specs, accumulating names into `seen`.
///
/// Pass the same `seen` for all archive flags to catch duplicates across them.
fn parse_archive_specs(
    raw: &[String],
    seen: &mut Vec<String>,
) -> Result<Vec<archive::ArchiveSpec>> {
    let mut specs = Vec::new();
    for arg in raw {
        let spec = archive::ArchiveSpec::parse(arg)?;
        let key = spec.mount.as_str().to_string();
        if seen.contains(&key) {
            anyhow::bail!(
                "duplicate archive name '{}'; use NAME: prefix to disambiguate",
                key
            );
        }
        seen.push(key);
        specs.push(spec);
    }
    Ok(specs)
}

/// Parse a list of `--dynamic-empty` specs, accumulating names into `seen`.
fn parse_empty_specs(raw: &[String], seen: &mut Vec<String>) -> Result<Vec<archive::EmptySpec>> {
    let mut specs = Vec::new();
    for arg in raw {
        let spec = archive::EmptySpec::parse(arg)?;
        let key = spec.mount.as_str().to_string();
        if seen.contains(&key) {
            anyhow::bail!(
                "duplicate archive name '{}'; use a different name to disambiguate",
                key
            );
        }
        seen.push(key);
        specs.push(spec);
    }
    Ok(specs)
}

/// Resolve a `--run PATH` argument to an absolute executable path.
///
/// Rules (from the spec):
/// - `path` must be relative (no leading `/`) or start with `/run/fuselage/`
/// - the first path component must be the name of a mounted dynamic or static archive
/// - the rest of the path must resolve to an executable file under that archive's root
///
/// Dynamic archives are checked first; static archives second.
fn resolve_run_path(
    path: &str,
    dynamic_specs: &[archive::ArchiveSpec],
    empty_specs: &[archive::EmptySpec],
    static_specs: &[archive::ArchiveSpec],
    dynamic_root: &Path,
    static_root: &Path,
) -> Result<String> {
    let p = Path::new(path);

    // Fixed paths under /run/fuselage/ are passed through directly.
    if let Some(rest) = path.strip_prefix("/run/fuselage/") {
        let name = rest.split('/').next().unwrap_or("");
        let is_dynamic = dynamic_specs.iter().any(|s| s.mount.as_str() == name)
            || empty_specs.iter().any(|s| s.mount.as_str() == name);
        let is_static = static_specs.iter().any(|s| s.mount.as_str() == name);
        if !is_dynamic && !is_static {
            anyhow::bail!("--run: /run/fuselage/{name:?} does not match any mounted archive name");
        }
        return check_executable(p);
    }

    if p.is_absolute() {
        anyhow::bail!("--run path must be relative or under /run/fuselage/, got: {path:?}");
    }

    let mut components = p.components();
    let first = match components.next() {
        Some(std::path::Component::Normal(c)) => c.to_string_lossy().into_owned(),
        _ => anyhow::bail!("--run path must begin with an archive name, got: {path:?}"),
    };

    // Determine which root to look in.
    let root = if dynamic_specs.iter().any(|s| s.mount.as_str() == first)
        || empty_specs.iter().any(|s| s.mount.as_str() == first)
    {
        dynamic_root
    } else if static_specs.iter().any(|s| s.mount.as_str() == first) {
        static_root
    } else {
        anyhow::bail!(
            "--run: first path component {first:?} does not match any mounted archive name"
        );
    };

    check_executable(&root.join(path))
}

/// Verify that `full` exists, is a regular file, and is executable; return its path as a String.
fn check_executable(full: &Path) -> Result<String> {
    use std::os::unix::fs::PermissionsExt;

    if !full.exists() {
        anyhow::bail!("--run: path does not exist: {}", full.display());
    }
    if !full.is_file() {
        anyhow::bail!("--run: path is not a file: {}", full.display());
    }
    let mode = std::fs::metadata(full)?.permissions().mode();
    if mode & 0o111 == 0 {
        anyhow::bail!("--run: file is not executable: {}", full.display());
    }
    Ok(full.to_string_lossy().into_owned())
}

/// Fork, exec `prog` with `argv` in the child, wait for it in the parent,
/// then clean up the procdir and any fixed-path mounts before exiting.
///
/// If `drop_to` is `Some((uid, gid))`, the child permanently drops privileges
/// to that uid/gid before exec (setuid mode). The parent retains its privileges
/// for cleanup.
fn run_with_cleanup(
    prog: &CString,
    argv: &[CString],
    procdir: &Path,
    drop_to: Option<(Uid, Gid)>,
    cache_dir: &Path,
    fixed_paths: Vec<PathBuf>,
) -> Result<()> {
    use nix::sys::wait::{WaitStatus, waitpid};
    use nix::unistd::{ForkResult, fork};

    match unsafe { fork() }.context("fork failed")? {
        ForkResult::Child => {
            if let Some((uid, gid)) = drop_to {
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
            // Best-effort cleanup of fixed-path mounts; failures are silent because
            // another concurrent fuselage invocation may still have the path mounted.
            for path in &fixed_paths {
                let _ = nix::mount::umount(path.as_path());
                let _ = std::fs::remove_dir(path);
            }
            procdir::spawn_cache_reaper(cache_dir);
            match status {
                WaitStatus::Exited(_, code) => std::process::exit(code),
                WaitStatus::Signaled(_, sig, _) => {
                    let _ = nix::sys::signal::raise(sig);
                    std::process::exit(128 + sig as i32);
                }
                _ => std::process::exit(1),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create an empty temp file and return its path as a String suitable
    /// for `parse_archive_specs`.
    fn tmp_file(dir: &Path, name: &str) -> String {
        let p = dir.join(name);
        fs::write(&p, b"PK\x03\x04").unwrap(); // minimal zip magic so ArchiveSpec resolves it
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn parse_specs_empty_input() {
        let mut seen = Vec::new();
        let specs = parse_archive_specs(&[], &mut seen).unwrap();
        assert!(specs.is_empty());
        assert!(seen.is_empty());
    }

    #[test]
    fn parse_specs_single_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = tmp_file(dir.path(), "data.zip");
        let mut seen = Vec::new();
        let specs = parse_archive_specs(&[path], &mut seen).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].mount.as_str(), "data");
        assert_eq!(seen, vec!["data"]);
    }

    #[test]
    fn parse_specs_duplicate_in_same_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let p1 = tmp_file(dir.path(), "data.zip");
        let p2 = tmp_file(dir.path(), "other.zip");
        // Use NAME: prefix to force the same name twice.
        let mut seen = Vec::new();
        let result = parse_archive_specs(&[format!("data:{p1}"), format!("data:{p2}")], &mut seen);
        assert!(result.is_err(), "duplicate name should be rejected");
    }

    #[test]
    fn parse_specs_duplicate_across_two_calls() {
        let dir = tempfile::TempDir::new().unwrap();
        let p1 = tmp_file(dir.path(), "data.zip");
        let p2 = tmp_file(dir.path(), "other.zip");
        let mut seen = Vec::new();
        parse_archive_specs(&[format!("shared:{p1}")], &mut seen).unwrap();
        let result = parse_archive_specs(&[format!("shared:{p2}")], &mut seen);
        assert!(
            result.is_err(),
            "duplicate name across dynamic/static should be rejected"
        );
    }

    #[test]
    fn parse_specs_two_distinct_names() {
        let dir = tempfile::TempDir::new().unwrap();
        let p1 = tmp_file(dir.path(), "alpha.zip");
        let p2 = tmp_file(dir.path(), "beta.zip");
        let mut seen = Vec::new();
        let specs = parse_archive_specs(&[p1, p2], &mut seen).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(seen, vec!["alpha", "beta"]);
    }

    #[test]
    fn parse_specs_fixed_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = tmp_file(dir.path(), "myapp.sfs");
        let mut seen = Vec::new();
        let specs = parse_archive_specs(&[format!("/run/fuselage/myapp:{p}")], &mut seen).unwrap();
        assert_eq!(specs.len(), 1);
        assert!(
            matches!(&specs[0].mount, archive::MountName::Fixed(path) if path == Path::new("/run/fuselage/myapp"))
        );
        assert_eq!(seen, vec!["myapp"]);
    }

    #[test]
    fn parse_specs_fixed_path_nested_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = tmp_file(dir.path(), "myapp.sfs");
        let mut seen = Vec::new();
        let result = parse_archive_specs(&[format!("/run/fuselage/a/b:{p}")], &mut seen);
        assert!(result.is_err(), "nested fixed path should be rejected");
    }

    #[test]
    fn parse_empty_spec_relative() {
        let mut seen = Vec::new();
        let specs = parse_empty_specs(&["mydir".to_string()], &mut seen).unwrap();
        assert_eq!(specs.len(), 1);
        assert!(matches!(&specs[0].mount, archive::MountName::Relative(n) if n == "mydir"));
    }

    #[test]
    fn parse_empty_spec_fixed() {
        let mut seen = Vec::new();
        let specs = parse_empty_specs(&["/run/fuselage/myapp".to_string()], &mut seen).unwrap();
        assert_eq!(specs.len(), 1);
        assert!(
            matches!(&specs[0].mount, archive::MountName::Fixed(p) if p == Path::new("/run/fuselage/myapp"))
        );
    }

    #[test]
    fn parse_empty_spec_other_absolute_rejected() {
        let mut seen = Vec::new();
        let result = parse_empty_specs(&["/tmp/mydir".to_string()], &mut seen);
        assert!(
            result.is_err(),
            "arbitrary absolute path should be rejected"
        );
    }
}
