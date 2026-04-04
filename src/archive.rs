use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Detected format of an archive file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Squashfs,
}

/// Parsed `[NAME:]FILE` archive specification.
pub struct ArchiveSpec {
    pub name: String,
    pub file: PathBuf,
}

impl ArchiveSpec {
    /// Parse a `[NAME:]FILE` argument.
    ///
    /// If the argument contains a colon, it is treated as `NAME:FILE` unless
    /// the whole string is itself a valid file path (handles colons in filenames).
    /// If no colon is present, the name is derived from the filename stem.
    pub fn parse(arg: &str) -> Result<Self> {
        if let Some(colon) = arg.find(':') {
            // The whole arg might be a file that happens to contain a colon.
            let whole = Path::new(arg);
            if whole.is_file() {
                return Ok(Self {
                    name: stem(arg),
                    file: whole.canonicalize().with_context(|| {
                        format!("failed to resolve path {}", whole.display())
                    })?,
                });
            }
            // Otherwise treat it as NAME:FILE.
            let name = arg[..colon].to_string();
            let file = Path::new(&arg[colon + 1..]);
            if !file.is_file() {
                anyhow::bail!("archive file not found: {}", file.display());
            }
            Ok(Self {
                name,
                file: file.canonicalize().with_context(|| {
                    format!("failed to resolve path {}", file.display())
                })?,
            })
        } else {
            let file = Path::new(arg);
            if !file.is_file() {
                anyhow::bail!("archive file not found: {}", file.display());
            }
            Ok(Self {
                name: stem(arg),
                file: file.canonicalize().with_context(|| {
                    format!("failed to resolve path {}", file.display())
                })?,
            })
        }
    }
}

/// Derive an archive name from a file path by stripping the directory
/// component and known archive extensions (`.sfs`, `.zip`, `.b64`).
fn stem(path: &str) -> String {
    let base = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let base = base.strip_suffix(".sfs").unwrap_or(&base);
    let base = base.strip_suffix(".zip").unwrap_or(base);
    let base = base.strip_suffix(".b64").unwrap_or(base);
    base.to_string()
}

/// Detect the archive format by reading the first 4 magic bytes.
pub fn detect_format(path: &Path) -> Result<ArchiveFormat> {
    let mut f = fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)
        .with_context(|| format!("failed to read magic bytes from {}", path.display()))?;
    match &magic {
        b"PK\x03\x04" => Ok(ArchiveFormat::Zip),
        b"hsqs" | b"sqsh" => Ok(ArchiveFormat::Squashfs),
        _ => anyhow::bail!(
            "{}: unrecognised archive format (magic {:02x?})",
            path.display(),
            magic
        ),
    }
}

/// Compute the SHA-256 hash of a file and return the first 16 hex characters.
pub fn compute_sha256(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    Ok(hex::encode(&result[..8])) // 8 bytes → 16 hex chars
}

/// Extract a zip archive into `dest`.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(archive)
        .with_context(|| format!("failed to open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("not a valid zip archive: {}", archive.display()))?;
    zip.extract(dest).with_context(|| {
        format!(
            "failed to extract {} into {}",
            archive.display(),
            dest.display()
        )
    })?;
    Ok(())
}

/// Extract a squashfs image into `dest` using the `backhand` library.
///
/// Creates the directory hierarchy, regular files (with permissions), and
/// symlinks. Device nodes, named pipes, and sockets are skipped.
pub fn extract_squashfs(sfs: &Path, dest: &Path) -> Result<()> {
    use backhand::{FilesystemReader, InnerNode};

    let file = BufReader::new(
        fs::File::open(sfs).with_context(|| format!("failed to open {}", sfs.display()))?,
    );
    let filesystem = FilesystemReader::from_reader(file)
        .with_context(|| format!("failed to read squashfs image {}", sfs.display()))?;

    for node in filesystem.files() {
        // Strip the leading "/" from the stored fullpath.
        let rel = node.fullpath.strip_prefix("/").unwrap_or(&node.fullpath);
        let out = dest.join(rel);

        match &node.inner {
            InnerNode::Dir(_) => {
                if out != dest {
                    fs::create_dir_all(&out)
                        .with_context(|| format!("failed to create dir {}", out.display()))?;
                }
            }
            InnerNode::File(file_reader) => {
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out_file = fs::File::create(&out)
                    .with_context(|| format!("failed to create {}", out.display()))?;
                let mut reader = filesystem.file(file_reader).reader();
                std::io::copy(&mut reader, &mut out_file)
                    .with_context(|| format!("failed to write {}", out.display()))?;
                fs::set_permissions(
                    &out,
                    fs::Permissions::from_mode(node.header.permissions as u32),
                )?;
            }
            InnerNode::Symlink(sym) => {
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                std::os::unix::fs::symlink(&sym.link, &out)
                    .with_context(|| format!("failed to create symlink {}", out.display()))?;
            }
            // Skip device nodes, named pipes, sockets — not relevant for archive contents.
            _ => {}
        }
    }
    Ok(())
}

/// Convert a zip archive to a squashfs image at `sfs_dest` by extracting to
/// `tmp_dir` and running `mksquashfs`.
///
/// Returns `true` if the squashfs was built successfully, or `false` if
/// `mksquashfs` is not installed (caller should fall back to a directory cache).
/// Returns an error if mksquashfs was found but failed.
pub fn zip_to_squashfs(zip: &Path, sfs_dest: &Path, tmp_dir: &Path) -> Result<bool> {
    // Check whether mksquashfs is on PATH before doing any work.
    if std::process::Command::new("mksquashfs")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        return Ok(false);
    }

    extract_zip(zip, tmp_dir)?;

    let status = std::process::Command::new("mksquashfs")
        .args([
            tmp_dir.as_os_str(),
            sfs_dest.as_os_str(),
            "-comp".as_ref(),
            "zstd".as_ref(),
            "-Xcompression-level".as_ref(),
            "1".as_ref(),
            "-noappend".as_ref(),
            "-quiet".as_ref(),
        ])
        .status()
        .context("failed to run mksquashfs")?;

    if !status.success() {
        anyhow::bail!("mksquashfs exited with status {:?}", status.code());
    }
    Ok(true)
}
