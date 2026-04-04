use anyhow::{Context, Result};
use nix::mount::{MntFlags, MsFlags, mount, umount2};
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
        let meta =
            fs::metadata(home).with_context(|| format!("failed to stat {}", home.display()))?;
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
        fs::create_dir_all(home).with_context(|| format!("failed to create {}", home.display()))?;
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
    for entry in
        fs::read_dir(&procdirs).with_context(|| format!("failed to read {}", procdirs.display()))?
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

/// Returns `~/.fuselage/cache/`.
pub fn cache_dir(home: &Path) -> PathBuf {
    home.join("cache")
}

/// Attach `sfs` to a fresh loop device and mount it read-only on `dest`.
///
/// Uses `LO_FLAGS_AUTOCLEAR` so the loop device detaches itself automatically
/// when the mount is removed (i.e. when the namespace is destroyed on exit).
/// No explicit cleanup is needed.
///
/// Requires `CAP_SYS_ADMIN` (real root or setuid-root mode).
pub fn loop_mount_sfs(sfs: &Path, dest: &Path) -> Result<()> {
    use loopdev::LoopControl;

    let ctrl = LoopControl::open().context("failed to open /dev/loop-control")?;
    let dev = ctrl.next_free().context("no free loop device available")?;

    dev.with()
        .read_only(true)
        .autoclear(true)
        .attach(sfs)
        .with_context(|| format!("failed to attach {} to loop device", sfs.display()))?;

    let loop_path = dev
        .path()
        .ok_or_else(|| anyhow::anyhow!("loop device has no path"))?;

    mount(
        Some(&loop_path),
        dest,
        Some("squashfs"),
        MsFlags::MS_RDONLY,
        None::<&str>,
    )
    .with_context(|| {
        format!(
            "failed to mount squashfs {} on {}",
            sfs.display(),
            dest.display()
        )
    })?;

    // `dev` drops here; autoclear fires when the mount is gone.
    Ok(())
}

/// Bind-mount `src` onto `dest` and then remount `dest` read-only.
///
/// Use this to expose an external directory (e.g. the cache) as a read-only
/// mount point inside the namespace.
pub fn bind_mount_readonly_from(src: &Path, dest: &Path) -> Result<()> {
    mount(
        Some(src),
        dest,
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .with_context(|| {
        format!(
            "bind-mount from {} to {} failed",
            src.display(),
            dest.display()
        )
    })?;

    mount(
        None::<&str>,
        dest,
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY,
        None::<&str>,
    )
    .with_context(|| format!("remount read-only failed on {}", dest.display()))?;

    Ok(())
}

/// Bind-mount `path` onto itself and then remount it read-only.
///
/// A bind-mount is required first because the kernel won't let you remount
/// a plain directory read-only — only an existing mount point.
pub fn bind_mount_readonly(path: &Path) -> Result<()> {
    bind_mount_readonly_from(path, path)
}

/// Recursively chown a directory tree to `uid`/`gid`.
///
/// Used in setuid mode to hand ownership of the tmpfs contents to the real user
/// after all dirs and extracted archives have been created as root.
pub fn chown_recursive(path: &Path, uid: nix::unistd::Uid, gid: nix::unistd::Gid) -> Result<()> {
    nix::unistd::chown(path, Some(uid), Some(gid))
        .with_context(|| format!("chown failed on {}", path.display()))?;
    if path.is_dir() {
        for entry in
            fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))?
        {
            chown_recursive(&entry?.path(), uid, gid)?;
        }
    }
    Ok(())
}

/// Lazily unmount the tmpfs and remove the now-empty procdir.
///
/// Errors are silently ignored since this is best-effort cleanup.
pub fn cleanup_procdir(procdir: &Path) {
    let _ = umount2(procdir, MntFlags::MNT_DETACH);
    let _ = fs::remove_dir(procdir);
}

/// Touch a cache sentinel file to record the current time as last-use time.
///
/// Sentinel files are always empty, so overwriting with zero bytes is safe
/// and portably updates the mtime without requiring utimensat(2).
pub fn touch_sentinel(path: &Path) -> Result<()> {
    fs::write(path, b"").with_context(|| format!("failed to touch sentinel {}", path.display()))
}

/// Spawn a double-forked background process that evicts stale cache entries.
///
/// The reaper runs after the parent has already exited (`std::process::exit`
/// is called after this returns), so no explicit wait is needed for the
/// grandchild — it is re-parented to init.
///
/// Expiry threshold is read from `FUSELAGE_CACHE_MAX_AGE_DAYS` (default 30).
/// Setting it to `0` disables reaping entirely.
///
/// Errors are silently ignored: the cache is a performance aid, not a
/// critical resource.
pub fn spawn_cache_reaper(cache_dir: &Path) {
    if !cache_dir.exists() {
        return;
    }

    let max_age_days: u64 = std::env::var("FUSELAGE_CACHE_MAX_AGE_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    if max_age_days == 0 {
        return;
    }

    let max_age_secs = max_age_days.saturating_mul(86400);
    let cache_dir = cache_dir.to_path_buf();

    // Double-fork: intermediate child exits immediately, grandchild is
    // re-parented to init and performs the reap without blocking the parent.
    unsafe {
        use nix::unistd::{ForkResult, fork};
        match fork() {
            Ok(ForkResult::Child) => {
                match fork() {
                    Ok(ForkResult::Child) => {
                        reap_cache(&cache_dir, max_age_secs);
                        std::process::exit(0);
                    }
                    _ => std::process::exit(0), // intermediate child exits immediately
                }
            }
            Ok(ForkResult::Parent { child }) => {
                // Wait for intermediate child (exits immediately — no zombie).
                let _ = nix::sys::wait::waitpid(child, None);
            }
            Err(_) => {} // ignore fork errors
        }
    }
}

/// Recursively ensure every directory in `path` is writable by the owner.
///
/// Required before `remove_dir_all` when the tree may contain directories
/// extracted from archives with mode 0555 (no write bit).  Only the owner
/// bits are touched; group/other permissions are left unchanged.
fn make_dir_tree_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        if meta.is_dir() {
            let mode = meta.permissions().mode();
            if mode & 0o200 == 0 {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700));
            }
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    make_dir_tree_writable(&entry.path());
                }
            }
        }
    }
}

/// Evict stale entries from the cache directory.
///
/// Rules:
/// - `.complete` sentinels older than `max_age_secs` are removed along with
///   their `.sfs` file and extracted directory.
/// - Sentinels touched within the last 60 seconds are always kept (recency
///   guard prevents racing with a concurrent fuselage that just wrote them).
/// - Orphaned `.sfs` files (no matching `.complete`) older than 1 hour are
///   removed (interrupted builds).
pub(crate) fn reap_cache(cache_dir: &Path, max_age_secs: u64) {
    const RECENCY_GUARD_SECS: u64 = 60;
    const ORPHAN_AGE_SECS: u64 = 3600;

    let now = std::time::SystemTime::now();

    let entries = match fs::read_dir(cache_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut sfs_stems: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut to_evict: Vec<String> = Vec::new(); // stems to evict

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().into_owned();

        if let Some(stem) = name_str.strip_suffix(".complete") {
            let age = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| now.duration_since(t).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            if age >= RECENCY_GUARD_SECS && age > max_age_secs {
                to_evict.push(stem.to_string());
            }
        } else if let Some(stem) = name_str.strip_suffix(".sfs") {
            sfs_stems.insert(stem.to_string());
        }
    }

    // Remove stale entries (sfs + directory + sentinel).
    for stem in &to_evict {
        let _ = fs::remove_file(cache_dir.join(format!("{stem}.sfs")));
        let dir = cache_dir.join(stem);
        if dir.exists() {
            make_dir_tree_writable(&dir);
            let _ = fs::remove_dir_all(&dir);
        }
        let _ = fs::remove_file(cache_dir.join(format!("{stem}.complete")));
    }

    // Remove orphaned .sfs files (no sentinel) that are old enough.
    let evicted: std::collections::HashSet<&str> = to_evict.iter().map(String::as_str).collect();
    for stem in &sfs_stems {
        if evicted.contains(stem.as_str()) {
            continue; // already removed above
        }
        let sentinel = cache_dir.join(format!("{stem}.complete"));
        if sentinel.exists() {
            continue; // sentinel present — not orphaned
        }
        let sfs = cache_dir.join(format!("{stem}.sfs"));
        let age = fs::metadata(&sfs)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| now.duration_since(t).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if age > ORPHAN_AGE_SECS {
            let _ = fs::remove_file(&sfs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{FileTime, set_file_mtime};
    use std::time::{Duration, SystemTime};

    /// Write an empty file at `dir/name` and optionally backdate its mtime by
    /// `age_secs` seconds relative to now.
    fn touch(dir: &Path, name: &str, age_secs: u64) {
        let path = dir.join(name);
        fs::write(&path, b"").unwrap();
        if age_secs > 0 {
            let mtime = SystemTime::now() - Duration::from_secs(age_secs);
            set_file_mtime(&path, FileTime::from_system_time(mtime)).unwrap();
        }
    }

    /// Create a directory at `dir/name` and optionally backdate its mtime.
    fn mkdir(dir: &Path, name: &str) {
        fs::create_dir_all(dir.join(name)).unwrap();
    }

    // ── Eviction of stale sentinels ───────────────────────────────────────────

    #[test]
    fn reap_evicts_old_sentinel_and_sfs() {
        let cache = tempfile::TempDir::new().unwrap();
        let d = cache.path();
        // Sentinel and sfs older than 31 days (beyond 60-second recency guard).
        touch(d, "aabbccdd11223344.complete", 31 * 86400);
        touch(d, "aabbccdd11223344.sfs", 31 * 86400);

        reap_cache(d, 30 * 86400);

        assert!(
            !d.join("aabbccdd11223344.complete").exists(),
            "sentinel should be removed"
        );
        assert!(
            !d.join("aabbccdd11223344.sfs").exists(),
            "sfs should be removed"
        );
    }

    #[test]
    fn reap_evicts_old_sentinel_and_directory() {
        let cache = tempfile::TempDir::new().unwrap();
        let d = cache.path();
        touch(d, "aabbccdd11223344.complete", 31 * 86400);
        mkdir(d, "aabbccdd11223344");

        reap_cache(d, 30 * 86400);

        assert!(!d.join("aabbccdd11223344.complete").exists());
        assert!(!d.join("aabbccdd11223344").exists());
    }

    // ── Recency guard ─────────────────────────────────────────────────────────

    #[test]
    fn reap_keeps_recent_sentinel() {
        let cache = tempfile::TempDir::new().unwrap();
        let d = cache.path();
        // 30 seconds old — within the 60-second recency guard.
        touch(d, "aabbccdd11223344.complete", 30);
        touch(d, "aabbccdd11223344.sfs", 30);

        // Use a very short max_age so age > max_age is true, but recency guard fires.
        reap_cache(d, 10);

        assert!(
            d.join("aabbccdd11223344.complete").exists(),
            "recent sentinel must not be removed"
        );
        assert!(
            d.join("aabbccdd11223344.sfs").exists(),
            "recent sfs must not be removed"
        );
    }

    // ── Active entries (younger than max_age) ─────────────────────────────────

    #[test]
    fn reap_keeps_young_sentinel() {
        let cache = tempfile::TempDir::new().unwrap();
        let d = cache.path();
        // 10 days old — younger than 30-day threshold.
        touch(d, "aabbccdd11223344.complete", 10 * 86400);
        touch(d, "aabbccdd11223344.sfs", 10 * 86400);

        reap_cache(d, 30 * 86400);

        assert!(d.join("aabbccdd11223344.complete").exists());
        assert!(d.join("aabbccdd11223344.sfs").exists());
    }

    // ── Orphaned .sfs files ───────────────────────────────────────────────────

    #[test]
    fn reap_removes_orphaned_sfs_older_than_one_hour() {
        let cache = tempfile::TempDir::new().unwrap();
        let d = cache.path();
        // No .complete sentinel; sfs is 2 hours old.
        touch(d, "aabbccdd11223344.sfs", 2 * 3600);

        reap_cache(d, 30 * 86400);

        assert!(
            !d.join("aabbccdd11223344.sfs").exists(),
            "old orphaned sfs should be removed"
        );
    }

    #[test]
    fn reap_keeps_orphaned_sfs_younger_than_one_hour() {
        let cache = tempfile::TempDir::new().unwrap();
        let d = cache.path();
        // No .complete sentinel; sfs is only 10 minutes old (in-progress build).
        touch(d, "aabbccdd11223344.sfs", 600);

        reap_cache(d, 30 * 86400);

        assert!(
            d.join("aabbccdd11223344.sfs").exists(),
            "young orphaned sfs should be kept"
        );
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn reap_nonexistent_dir_is_noop() {
        // Must not panic or error.
        reap_cache(std::path::Path::new("/nonexistent/cache"), 30 * 86400);
    }

    #[test]
    fn reap_empty_dir_is_noop() {
        let cache = tempfile::TempDir::new().unwrap();
        reap_cache(cache.path(), 30 * 86400);
    }
}
