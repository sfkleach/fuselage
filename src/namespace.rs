use anyhow::{Context, Result};
use nix::mount::{MsFlags, mount};
use nix::sched::{CloneFlags, unshare};
use nix::unistd::{Gid, Uid};
use std::fs;

/// Attempt the `unshare(2)` call that creates the namespace(s).
///
/// This is the only step that may fail for policy reasons (e.g. unprivileged
/// user namespaces disabled). It is therefore the only step that is safe to
/// recover from in `--extract=allow` mode. All subsequent setup steps are
/// fatal if they fail, because the process is already inside a namespace.
///
/// `is_privileged` should be `geteuid().is_root()`, evaluated *before* this
/// call; in the unprivileged path `getuid()` returns the overflow uid (65534)
/// after the user namespace is created but before uid_map is written.
pub fn try_unshare(is_privileged: bool) -> Result<()> {
    if is_privileged {
        unshare(CloneFlags::CLONE_NEWNS).context(
            "failed to create mount namespace; try running as root or with user namespace support",
        )?;
    } else {
        unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS).context(
            "failed to create user+mount namespace; \
             check that unprivileged user namespaces are enabled \
             (sysctl kernel.unprivileged_userns_clone=1)",
        )?;
    }
    Ok(())
}

/// Complete namespace setup after a successful `try_unshare`.
///
/// Writes uid/gid maps for unprivileged user namespaces and makes all
/// existing mounts private. Always fatal on failure: the process is already
/// inside a (partially configured) namespace with no clean way to undo it.
///
/// `uid` and `gid` must be the caller's real uid/gid captured *before*
/// `try_unshare` was called.
pub fn finish_namespace_setup(is_privileged: bool, uid: Uid, gid: Gid) -> Result<()> {
    if !is_privileged {
        // Kernel requires setgroups to be denied before writing gid_map
        // when called from an unprivileged process.
        fs::write("/proc/self/setgroups", "deny")
            .context("failed to write /proc/self/setgroups")?;
        fs::write("/proc/self/uid_map", format!("0 {} 1\n", uid))
            .context("failed to write /proc/self/uid_map")?;
        fs::write("/proc/self/gid_map", format!("0 {} 1\n", gid))
            .context("failed to write /proc/self/gid_map")?;
    }

    // Make all existing mounts private so our later mounts don't propagate out.
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .context("failed to set mount propagation to private")?;

    Ok(())
}

/// Enter a private mount namespace.
///
/// Convenience wrapper over `try_unshare` + `finish_namespace_setup` for
/// callers that treat all failures as fatal (i.e. `--extract=deny` and
/// `--extract=prefer` in privileged mode).
///
/// For unprivileged callers, also creates a user namespace so that
/// `mount(2)` is permitted. The caller is mapped to uid 0 inside the
/// namespace (user-namespace mode caveat: `id` shows uid=0, `sudo` won't work).
///
/// For root callers, only a mount namespace is created.
pub fn enter_namespace() -> Result<()> {
    let is_privileged = nix::unistd::geteuid().is_root();
    let uid = nix::unistd::getuid();
    let gid = nix::unistd::getgid();
    try_unshare(is_privileged)?;
    finish_namespace_setup(is_privileged, uid, gid)
}
