use anyhow::{Context, Result};
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Returns `~/.fuselage`, or the value of `$FUSELAGE_HOME` if set.
pub fn fuselage_home() -> PathBuf {
    if let Ok(val) = std::env::var("FUSELAGE_HOME") {
        return PathBuf::from(val);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".fuselage")
}

/// Verify or create the fuselage home directory.
///
/// Aborts if the directory exists but is owned by a different user.
pub fn setup_home(home: &Path) -> Result<()> {
    if home.exists() {
        let meta = fs::metadata(home)
            .with_context(|| format!("failed to stat {}", home.display()))?;
        let owner = meta.uid();
        let my_uid = nix::unistd::getuid().as_raw();
        if owner != my_uid {
            anyhow::bail!(
                "{} exists but is owned by uid {}, not {}",
                home.display(),
                owner,
                my_uid
            );
        }
    } else {
        fs::create_dir_all(home)
            .with_context(|| format!("failed to create {}", home.display()))?;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set permissions on {}", home.display()))?;
    }
    Ok(())
}

/// Remove stale `procdirs/<pid>/` entries whose process no longer exists.
pub fn clean_stale_procdirs(home: &Path) -> Result<()> {
    let procdirs = home.join("procdirs");
    if !procdirs.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&procdirs)
        .with_context(|| format!("failed to read {}", procdirs.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if let Ok(pid) = name.to_string_lossy().parse::<i32>() {
            let pid = nix::unistd::Pid::from_raw(pid);
            if nix::sys::signal::kill(pid, None).is_err() {
                let _ = fs::remove_dir(entry.path());
            }
        }
    }
    Ok(())
}

/// Create `procdirs/<pid>/` and return its path.
pub fn create_procdir(home: &Path) -> Result<PathBuf> {
    let pid = nix::unistd::getpid();
    let procdir = home.join("procdirs").join(pid.to_string());
    fs::create_dir_all(&procdir)
        .with_context(|| format!("failed to create procdir {}", procdir.display()))?;
    Ok(procdir)
}

/// Mount a tmpfs on `procdir` and create the `tmp/` subdirectory inside it.
///
/// Must be called after entering the mount namespace.
pub fn setup_procdir_in_namespace(procdir: &Path) -> Result<()> {
    mount(
        Some("fuselage-proc"),
        procdir,
        Some("tmpfs"),
        MsFlags::empty(),
        Some("mode=0700"),
    )
    .with_context(|| format!("failed to mount tmpfs on {}", procdir.display()))?;

    let tmpdir = procdir.join("tmp");
    fs::create_dir_all(&tmpdir)
        .with_context(|| format!("failed to create {}", tmpdir.display()))?;

    Ok(())
}

/// Lazily unmount the tmpfs and remove the now-empty procdir.
///
/// Errors are silently ignored since this is best-effort cleanup.
pub fn cleanup_procdir(procdir: &Path) {
    let _ = umount2(procdir, MntFlags::MNT_DETACH);
    let _ = fs::remove_dir(procdir);
}
