use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<()> {
    let args = parse_args()?;
    run(&args)
}

struct Args {
    project: PathBuf,
    output: PathBuf,
    /// Python module to run with `-m`. Defaults to the package name.
    module: Option<String>,
    /// Keep the intermediate squashfs at this explicit path.
    squashfs: Option<PathBuf>,
    /// Extra arguments forwarded verbatim to `uv sync`.
    extra_uv_args: Vec<String>,
    /// Include dev dependencies in the bundle. Defaults to false (--no-dev).
    dev: bool,
}

fn parse_args() -> Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    parse_args_from(&raw)
}

fn parse_args_from(raw: &[String]) -> Result<Args> {
    let mut project: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut module: Option<String> = None;
    let mut squashfs: Option<PathBuf> = None;
    let mut extra_uv_args: Vec<String> = Vec::new();
    let mut dev = false;

    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        if let Some(val) = arg.strip_prefix("--project=") {
            project = Some(PathBuf::from(val));
        } else if arg == "--project" {
            i += 1;
            let val = raw.get(i).context("--project requires a value")?;
            project = Some(PathBuf::from(val));
        } else if let Some(val) = arg.strip_prefix("--output=") {
            output = Some(PathBuf::from(val));
        } else if arg == "--output" {
            i += 1;
            let val = raw.get(i).context("--output requires a value")?;
            output = Some(PathBuf::from(val));
        } else if let Some(val) = arg.strip_prefix("--module=") {
            module = Some(val.to_owned());
        } else if arg == "--module" {
            i += 1;
            let val = raw.get(i).context("--module requires a value")?;
            module = Some(val.clone());
        } else if let Some(val) = arg.strip_prefix("--squashfs=") {
            squashfs = Some(PathBuf::from(val));
        } else if arg == "--squashfs" {
            i += 1;
            let val = raw.get(i).context("--squashfs requires a value")?;
            squashfs = Some(PathBuf::from(val));
        } else if let Some(val) = arg.strip_prefix("--uv-arg=") {
            extra_uv_args.push(val.to_owned());
        } else if arg == "--uv-arg" {
            i += 1;
            let val = raw.get(i).context("--uv-arg requires a value")?;
            extra_uv_args.push(val.clone());
        } else if arg == "--dev" {
            dev = true;
        } else {
            anyhow::bail!("unrecognised argument: {arg:?}");
        }
        i += 1;
    }

    let project = project.context("--project is required")?;
    let output = output.context("--output is required")?;

    if !project.join("pyproject.toml").is_file() {
        anyhow::bail!("no pyproject.toml found in {}", project.display());
    }

    Ok(Args {
        project,
        output,
        module,
        squashfs,
        extra_uv_args,
        dev,
    })
}

fn run(args: &Args) -> Result<()> {
    let package_name = resolve_package_name(args)?;
    let module = args.module.as_deref().unwrap_or(&package_name).to_owned();
    let mount_point = format!("/run/fuselage/{package_name}");

    // Squashfs is written outside the mount so it survives after fuselage exits.
    let squashfs_owned;
    let squashfs_path: &Path = match &args.squashfs {
        Some(p) => p.as_path(),
        None => {
            squashfs_owned =
                std::env::temp_dir().join(format!("_uv_bundle_{}.sfs", std::process::id()));
            squashfs_owned.as_path()
        }
    };

    // Steps 1–3: mount, sync, compress — all inside a single fuselage invocation.
    run_mount_sync_compress(
        &mount_point,
        &args.project,
        squashfs_path,
        &args.extra_uv_args,
        args.dev,
    )?;

    // Step 4: pack squashfs + baked args into the output ELF.
    let pack_result = run_fuselage_bundle(squashfs_path, &args.output, &mount_point, &module);

    // Remove temp squashfs unless the caller asked to keep it at an explicit path.
    if args.squashfs.is_none() {
        if let Err(e) = std::fs::remove_file(squashfs_path) {
            eprintln!(
                "warning: could not remove temp squashfs {}: {e}",
                squashfs_path.display()
            );
        }
    }

    pack_result
}

/// Read the package name from `pyproject.toml`.
fn resolve_package_name(args: &Args) -> Result<String> {
    let pyproject = args.project.join("pyproject.toml");
    let content = std::fs::read_to_string(&pyproject)
        .with_context(|| format!("failed to read {}", pyproject.display()))?;

    let doc: toml::Value = content
        .parse()
        .with_context(|| format!("failed to parse {}", pyproject.display()))?;

    let name = doc
        .get("project")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .with_context(|| format!("{}: missing [project].name", pyproject.display()))?;

    // PEP 625 normalisation: hyphens and dots become underscores in the importable name.
    Ok(name.replace(['-', '.'], "_"))
}

/// Wrap a string in single quotes, escaping any embedded single quotes.
///
/// This is the standard POSIX shell quoting transform: a single quote inside
/// a single-quoted string is represented as '\'' (close quote, escaped quote,
/// reopen quote).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Run fuselage --dynamic-empty, uv sync, and mksquashfs as a single pipeline.
///
/// All three steps share the same mount namespace: uv sync populates the mount
/// and mksquashfs compresses it before fuselage tears it down.
fn run_mount_sync_compress(
    mount_point: &str,
    project: &Path,
    squashfs_path: &Path,
    extra_uv_args: &[String],
    dev: bool,
) -> Result<()> {
    let project_str = project
        .to_str()
        .with_context(|| format!("project path is not valid UTF-8: {}", project.display()))?;
    let squashfs_str = squashfs_path.to_str().with_context(|| {
        format!(
            "squashfs path is not valid UTF-8: {}",
            squashfs_path.display()
        )
    })?;

    let mut uv_flags: Vec<String> = Vec::new();
    if !dev {
        uv_flags.push("--no-dev".to_owned());
    }
    uv_flags.extend(extra_uv_args.iter().map(|a| shell_quote(a)));
    let uv_flags_str = uv_flags.join(" ");

    // uv sync installs into the mount via UV_PROJECT_ENVIRONMENT so the venv
    // ends up inside the squashfs rather than alongside the source tree.
    let sh_cmd = format!(
        "UV_PROJECT_ENVIRONMENT={} uv sync --project {} {} && mksquashfs {} {} -noappend -quiet",
        shell_quote(&format!("{mount_point}/.venv")),
        shell_quote(project_str),
        uv_flags_str,
        shell_quote(mount_point),
        shell_quote(squashfs_str),
    );

    let status = Command::new("fuselage")
        .arg(format!("--dynamic-empty={mount_point}"))
        .arg("--")
        .arg("sh")
        .arg("-c")
        .arg(&sh_cmd)
        .status()
        .context("failed to run fuselage — is it installed and on PATH?")?;

    if !status.success() {
        anyhow::bail!("fuselage/uv sync/mksquashfs pipeline failed");
    }

    Ok(())
}

/// Run fuselage-bundle to produce the final self-executing ELF.
fn run_fuselage_bundle(
    squashfs_path: &Path,
    output: &Path,
    mount_point: &str,
    module: &str,
) -> Result<()> {
    let status = Command::new("fuselage-bundle")
        .arg(format!("--archive={}", squashfs_path.display()))
        .arg(format!("--output={}", output.display()))
        .arg("--")
        .arg(format!("--static={mount_point}:/proc/self/exe"))
        .arg(format!("--run={mount_point}/.venv/bin/python"))
        .arg("--")
        .arg("-m")
        .arg(module)
        .status()
        .context("failed to run fuselage-bundle — is it installed and on PATH?")?;

    if !status.success() {
        anyhow::bail!("fuselage-bundle failed");
    }

    println!("uv-bundle: wrote {}", output.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ss(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn write_pyproject(dir: &Path, name: &str) {
        std::fs::write(
            dir.join("pyproject.toml"),
            format!("[project]\nname = \"{name}\"\n"),
        )
        .unwrap();
    }

    // ── shell_quote ───────────────────────────────────────────────────────────

    #[test]
    fn shell_quote_plain() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn shell_quote_spaces() {
        assert_eq!(shell_quote("my project"), "'my project'");
    }

    #[test]
    fn shell_quote_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_quote_empty() {
        assert_eq!(shell_quote(""), "''");
    }

    // ── parse_args_from ───────────────────────────────────────────────────────

    #[test]
    fn parse_args_basic() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out/app",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.output, PathBuf::from("/out/app"));
        assert!(args.module.is_none());
    }

    #[test]
    fn parse_args_space_separated_flags() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let proj = tmp.path().to_str().unwrap().to_owned();
        let raw = ss(&["--project", &proj, "--output", "/out/app"]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.output, PathBuf::from("/out/app"));
    }

    #[test]
    fn parse_args_module_override() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--module=mymod",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.module.as_deref(), Some("mymod"));
    }

    #[test]
    fn parse_args_module_space_separated() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--module",
            "mymod",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.module.as_deref(), Some("mymod"));
    }

    #[test]
    fn parse_args_uv_args_accumulated() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--uv-arg=--frozen",
            "--uv-arg=--no-dev",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.extra_uv_args, vec!["--frozen", "--no-dev"]);
    }

    #[test]
    fn parse_args_uv_arg_space_separated() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--uv-arg",
            "--frozen",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.extra_uv_args, vec!["--frozen"]);
    }

    #[test]
    fn parse_args_squashfs_explicit() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--squashfs=/tmp/a.sfs",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert_eq!(args.squashfs.as_deref(), Some(Path::new("/tmp/a.sfs")));
    }

    #[test]
    fn parse_args_missing_project_is_error() {
        let raw = ss(&["--output=/out"]);
        assert!(parse_args_from(&raw).is_err());
    }

    #[test]
    fn parse_args_missing_output_is_error() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[&format!("--project={}", tmp.path().display())]);
        assert!(parse_args_from(&raw).is_err());
    }

    #[test]
    fn parse_args_unknown_flag_is_error() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--bogus=x",
        ]);
        assert!(parse_args_from(&raw).is_err());
    }

    #[test]
    fn parse_args_dev_defaults_to_false() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert!(!args.dev);
    }

    #[test]
    fn parse_args_dev_flag_sets_dev_true() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "x");
        let raw = ss(&[
            &format!("--project={}", tmp.path().display()),
            "--output=/out",
            "--dev",
        ]);
        let args = parse_args_from(&raw).unwrap();
        assert!(args.dev);
    }

    #[test]
    fn parse_args_no_pyproject_is_error() {
        // /tmp exists but has no pyproject.toml.
        let raw = ss(&["--project=/tmp", "--output=/out"]);
        assert!(parse_args_from(&raw).is_err());
    }

    // ── resolve_package_name ──────────────────────────────────────────────────

    fn args_with_project(dir: &Path) -> Args {
        Args {
            project: dir.to_path_buf(),
            output: PathBuf::from("/out"),
            module: None,
            squashfs: None,
            extra_uv_args: vec![],
            dev: false,
        }
    }

    #[test]
    fn resolve_name_reads_pyproject() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "myapp");
        let args = args_with_project(tmp.path());
        assert_eq!(resolve_package_name(&args).unwrap(), "myapp");
    }

    #[test]
    fn resolve_name_normalises_hyphens() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "my-cool-app");
        let args = args_with_project(tmp.path());
        assert_eq!(resolve_package_name(&args).unwrap(), "my_cool_app");
    }

    #[test]
    fn resolve_name_normalises_dots() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(tmp.path(), "my.pkg");
        let args = args_with_project(tmp.path());
        assert_eq!(resolve_package_name(&args).unwrap(), "my_pkg");
    }

    #[test]
    fn resolve_name_missing_project_section_is_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "[tool.x]\nfoo=\"bar\"\n").unwrap();
        let args = args_with_project(tmp.path());
        assert!(resolve_package_name(&args).is_err());
    }

    #[test]
    fn resolve_name_invalid_toml_is_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "not valid toml {{{{").unwrap();
        let args = args_with_project(tmp.path());
        assert!(resolve_package_name(&args).is_err());
    }

    #[test]
    fn resolve_name_missing_name_key_is_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pyproject.toml"),
            "[project]\nversion=\"1.0\"\n",
        )
        .unwrap();
        let args = args_with_project(tmp.path());
        assert!(resolve_package_name(&args).is_err());
    }
}
