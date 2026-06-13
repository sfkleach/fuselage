use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// fuselage-bundle: bundle a squashfs archive and a fuselage invocation into a
/// self-executing ELF binary.
///
/// Usage:
///   fuselage-bundle --archive=SQUASHFS --output=BINARY -- [FUSELAGE_ARGS...]
///
/// Everything after -- is stored verbatim as the baked-in fuselage argument
/// list. The resulting binary, when executed as `BINARY ARGS...`, does:
///
///   exec fuselage FUSELAGE_ARGS... -- ARGS...
///
/// /proc/self/exe in FUSELAGE_ARGS is substituted at runtime with the
/// resolved absolute path of the binary itself.
fn main() -> Result<()> {
    let (archive, output, fuselage_args) = parse_args()?;
    bundle(&archive, &output, &fuselage_args)
}

/// Parse command-line arguments.
///
/// Returns `(archive_path, output_path, fuselage_args)`.
fn parse_args() -> Result<(PathBuf, PathBuf, Vec<String>)> {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut archive: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
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

    Ok((archive, output, fuselage_args))
}

/// Generate, compile, and assemble the output binary.
fn bundle(archive: &Path, output: &Path, fuselage_args: &[String]) -> Result<()> {
    let build_dir = Path::new("_build");
    std::fs::create_dir_all(build_dir).context("failed to create _build/")?;

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

/// Compile `src` with gcc into a statically linked binary at `dest`.
fn compile_stub(src: &Path, dest: &Path) -> Result<()> {
    let status = Command::new("gcc")
        .args([
            "-static",
            "-O2",
            "-o",
            dest.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .status()
        .context("failed to run gcc — is gcc installed?")?;

    if !status.success() {
        anyhow::bail!("gcc failed to compile {}", src.display());
    }

    Ok(())
}

/// Assemble the output binary: stub ELF + zero padding to 4096-byte alignment
/// + squashfs image.
fn assemble(output: &Path, stub: &Path, squashfs: &Path) -> Result<()> {
    let stub_bytes =
        std::fs::read(stub).with_context(|| format!("failed to read {}", stub.display()))?;
    let sfs_bytes = std::fs::read(squashfs)
        .with_context(|| format!("failed to read {}", squashfs.display()))?;

    // Verify the squashfs magic.
    if sfs_bytes.len() < 4 || (&sfs_bytes[..4] != b"hsqs" && &sfs_bytes[..4] != b"sqsh") {
        anyhow::bail!(
            "{}: does not look like a squashfs image (bad magic)",
            squashfs.display()
        );
    }

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

    f.write_all(&sfs_bytes)
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
