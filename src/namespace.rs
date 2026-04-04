use anyhow::{Context, Result};
use nix::mount::{MsFlags, mount};
use nix::sched::{CloneFlags, unshare};
use std::fs;

/// Enter a private mount namespace.
///
/// For unprivileged callers, also creates a user namespace so that
/// `mount(2)` is permitted. The caller is mapped to uid 0 inside the
/// namespace (user-namespace mode caveat: `id` shows uid 0, `sudo` won't work).
///
/// For root callers, only a mount namespace is created.
pub fn enter_namespace() -> Result<()> {
    // Use effective uid: a setuid-root binary has euid=0 but ruid=real_user.
    // In that case we want a plain mount namespace (we already have CAP_SYS_ADMIN),
    // not a user namespace.
    let euid = nix::unistd::geteuid();
    let uid = nix::unistd::getuid();
    let gid = nix::unistd::getgid();

    if euid.is_root() {
        unshare(CloneFlags::CLONE_NEWNS).context(
            "failed to create mount namespace; try running as root or with user namespace support",
        )?;
    } else {
        unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS).context(
            "failed to create user+mount namespace; \
             check that unprivileged user namespaces are enabled \
             (sysctl kernel.unprivileged_userns_clone=1)",
        )?;

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
