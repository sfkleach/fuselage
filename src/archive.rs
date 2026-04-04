use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

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
/// component and known archive extensions (`.zip`, `.b64`).
fn stem(path: &str) -> String {
    let base = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let base = base.strip_suffix(".zip").unwrap_or(&base);
    let base = base.strip_suffix(".b64").unwrap_or(base);
    base.to_string()
}

/// Extract a zip archive into `dest`.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(archive)
        .with_context(|| format!("failed to open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("not a valid zip archive: {}", archive.display()))?;
    zip.extract(dest)
        .with_context(|| format!("failed to extract {} into {}", archive.display(), dest.display()))?;
    Ok(())
}
