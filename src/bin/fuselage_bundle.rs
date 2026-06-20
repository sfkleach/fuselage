use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// fuselage-bundle: bundle a squashfs archive and a fuselage invocation into a
/// self-executing ELF binary.
///
/// Usage:
///   fuselage-bundle [--build-dir=DIR] --archive=SQUASHFS --output=BINARY -- [FUSELAGE_ARGS...]
///
/// Everything after -- is stored verbatim as the baked-in fuselage argument
/// list. The resulting binary, when executed as `BINARY ARGS...`, does:
///
///   exec fuselage FUSELAGE_ARGS... -- ARGS...
///
/// /proc/self/exe in FUSELAGE_ARGS is substituted at runtime with the
/// resolved absolute path of the binary itself.
fn main() -> Result<()> {
    let args = parse_args()?;
    bundle(&args)
}

struct Args {
    archive: PathBuf,
    output: PathBuf,
    fuselage_args: Vec<String>,
    /// Explicit build directory. When absent a temporary directory is created
    /// under the system temp dir with a `_fuselage_bundle_<PID>` name.
    build_dir: Option<PathBuf>,
    /// Allow --build-dir to already exist; the pre-existing directory is never
    /// torn down (only directories created by this run are eligible for removal).
    exist_ok: bool,
    /// Suppress teardown of the build directory after bundling completes.
    keep: bool,
}

/// Parse command-line arguments.
fn parse_args() -> Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut archive: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut build_dir: Option<PathBuf> = None;
    let mut exist_ok = false;
    let mut keep = false;
    let mut fuselage_args: Vec<String> = Vec::new();
    let mut after_dashdash = false;

    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        if after_dashdash {
            fuselage_args.push(arg.clone());
        } else if arg == "--" {
            after_dashdash = true;
        } else if let Some(val) = arg.strip_prefix("--archive=") {
            archive = Some(PathBuf::from(val));
        } else if arg == "--archive" {
            i += 1;
            let val = raw.get(i).context("--archive requires a value")?;
            archive = Some(PathBuf::from(val));
        } else if let Some(val) = arg.strip_prefix("--output=") {
            output = Some(PathBuf::from(val));
        } else if arg == "--output" {
            i += 1;
            let val = raw.get(i).context("--output requires a value")?;
            output = Some(PathBuf::from(val));
        } else if let Some(val) = arg.strip_prefix("--build-dir=") {
            build_dir = Some(PathBuf::from(val));
        } else if arg == "--build-dir" {
            i += 1;
            let val = raw.get(i).context("--build-dir requires a value")?;
            build_dir = Some(PathBuf::from(val));
        } else if arg == "--exist-ok" {
            exist_ok = true;
        } else if arg == "--keep" {
            keep = true;
        } else {
            anyhow::bail!("unrecognised argument: {arg:?}");
        }
        i += 1;
    }

    let archive = archive.context("--archive is required")?;
    let output = output.context("--output is required")?;

    if !archive.is_file() {
        anyhow::bail!("archive file not found: {}", archive.display());
    }

    Ok(Args {
        archive,
        output,
        fuselage_args,
        build_dir,
        exist_ok,
        keep,
    })
}

/// Generate, compile, and assemble the output binary.
fn bundle(args: &Args) -> Result<()> {
    let build_dir = match &args.build_dir {
        Some(p) => p.clone(),
        None => std::env::temp_dir().join(format!("_fuselage_bundle_{}", std::process::id())),
    };

    let pre_existed = build_dir.exists();
    if pre_existed && !args.exist_ok {
        anyhow::bail!(
            "build dir already exists: {} (use --exist-ok to allow)",
            build_dir.display()
        );
    }
    // Only create (and later remove) the directory if this run is responsible for it.
    let we_created = !pre_existed;
    if we_created && !args.keep && args.output.starts_with(&build_dir) {
        anyhow::bail!(
            "--output is inside --build-dir and will be deleted on cleanup; \
             use --keep to preserve the build directory, or choose an output path outside it"
        );
    }
    if we_created {
        std::fs::create_dir_all(&build_dir)
            .with_context(|| format!("failed to create build dir {}", build_dir.display()))?;
    }

    let result = bundle_in(&build_dir, &args.archive, &args.output, &args.fuselage_args);

    if we_created && !args.keep {
        if let Err(e) = std::fs::remove_dir_all(&build_dir) {
            eprintln!(
                "warning: failed to remove build dir {}: {e}",
                build_dir.display()
            );
        }
    }

    result
}

/// Inner bundling logic that runs inside `build_dir`.
fn bundle_in(
    build_dir: &Path,
    archive: &Path,
    output: &Path,
    fuselage_args: &[String],
) -> Result<()> {
    // Step 1: generate stub.c from the template with the real argument list.
    let stub_c = build_dir.join("stub.c");
    generate_stub_c(&stub_c, fuselage_args)?;

    // Step 2: compile stub.c into a static ELF binary.
    let stub_bin = build_dir.join("stub");
    compile_stub(&stub_c, &stub_bin)?;

    // Step 3: assemble output = stub + padding + squashfs.
    assemble(output, &stub_bin, archive)?;

    // Step 4: make the output executable.
    set_executable(output)?;

    println!(
        "fuselage-bundle: wrote {} ({} bytes)",
        output.display(),
        std::fs::metadata(output)?.len()
    );

    Ok(())
}

/// Render stub_template.c with the baked-in argument list substituted.
fn generate_stub_c(dest: &Path, fuselage_args: &[String]) -> Result<()> {
    let template = include_str!("stub_template.c");

    // Build a C array body: one string literal per argument, comma-separated.
    let args_literal: String = fuselage_args
        .iter()
        .map(|a| format!("    {},\n", c_string_literal(a)))
        .collect();

    let source = template.replace("    FUSELAGE_BUNDLE_ARGS\n", &args_literal);

    std::fs::write(dest, source).with_context(|| format!("failed to write {}", dest.display()))?;

    Ok(())
}

/// Escape a Rust string into a C string literal including surrounding quotes.
fn c_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Compile `src` into a statically linked binary at `dest`.
///
/// Prefers musl-gcc when available: musl produces stripped static binaries of
/// ~50 KB versus ~700 KB for glibc, because musl was designed for efficient
/// static linking. Falls back to plain gcc if musl-gcc is not on PATH.
fn compile_stub(src: &Path, dest: &Path) -> Result<()> {
    let compiler = if which_compiler("musl-gcc") {
        "musl-gcc"
    } else {
        "gcc"
    };

    let status = Command::new(compiler)
        .args([
            "-static",
            "-O2",
            "-s",
            "-o",
            dest.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .status()
        .with_context(|| format!("failed to run {compiler} — is it installed?"))?;

    if !status.success() {
        anyhow::bail!("{compiler} failed to compile {}", src.display());
    }

    Ok(())
}

/// Return true if `name` resolves to an executable on PATH.
fn which_compiler(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// Assemble the output binary: stub ELF + zero padding to 4096-byte alignment
/// + squashfs image.
fn assemble(output: &Path, stub: &Path, squashfs: &Path) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};

    let stub_bytes =
        std::fs::read(stub).with_context(|| format!("failed to read {}", stub.display()))?;

    // Open and validate the squashfs magic before creating any output file,
    // so a bad input fails without side-effects.
    let mut sfs_file = std::fs::File::open(squashfs)
        .with_context(|| format!("failed to open {}", squashfs.display()))?;
    let mut magic = [0u8; 4];
    if sfs_file.read_exact(&mut magic).is_err() || (&magic != b"hsqs" && &magic != b"sqsh") {
        anyhow::bail!(
            "{}: does not look like a squashfs image (bad magic)",
            squashfs.display()
        );
    }
    sfs_file
        .seek(SeekFrom::Start(0))
        .with_context(|| format!("failed to seek {}", squashfs.display()))?;

    let stub_len = stub_bytes.len() as u64;
    let pad_len = align_up(stub_len, 4096) - stub_len;

    let mut f = std::fs::File::create(output)
        .with_context(|| format!("failed to create {}", output.display()))?;

    f.write_all(&stub_bytes)
        .with_context(|| format!("failed to write stub to {}", output.display()))?;

    // Padding bytes (zeroes) to bring the file to a page boundary.
    let padding = vec![0u8; pad_len as usize];
    f.write_all(&padding)
        .with_context(|| format!("failed to write padding to {}", output.display()))?;

    std::io::copy(&mut sfs_file, &mut f)
        .with_context(|| format!("failed to write squashfs to {}", output.display()))?;

    Ok(())
}

/// Round `value` up to the nearest multiple of `align` (which must be a power of two).
fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

/// Set the output file's executable bits (rwxr-xr-x).
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}
