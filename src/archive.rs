use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Detected format of an archive file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Squashfs,
    /// An ELF binary with an embedded squashfs image starting at the given byte offset.
    ElfSquashfs(u64),
}

/// The mount-point name for an archive, which is either a plain relative name
/// (mounted under the procdir) or a fixed absolute path under `/run/fuselage/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountName {
    /// A plain name such as `mydata`, mounted under the procdir.
    Relative(String),
    /// An absolute path `/run/fuselage/NAME`, mounted directly at that path.
    Fixed(PathBuf),
}

impl MountName {
    /// The plain name component (the last path segment for fixed paths).
    pub fn as_str(&self) -> &str {
        match self {
            MountName::Relative(s) => s.as_str(),
            MountName::Fixed(p) => p.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        }
    }
}

/// Parsed `[NAME:]FILE` or `/run/fuselage/NAME:FILE` archive specification.
pub struct ArchiveSpec {
    pub mount: MountName,
    pub file: PathBuf,
}

impl ArchiveSpec {
    /// Parse a `[NAME:]FILE` or `/run/fuselage/NAME:FILE` argument.
    ///
    /// If the argument contains a colon, it is treated as `NAME:FILE` unless
    /// the whole string is itself a valid file path (handles colons in filenames).
    /// If no colon is present, the name is derived from the filename stem.
    pub fn parse(arg: &str) -> Result<Self> {
        if let Some(colon) = arg.find(':') {
            // The whole arg might be a file that happens to contain a colon.
            let whole = Path::new(arg);
            if whole.is_file() {
                let mount = parse_mount_name(arg, arg)?;
                return Ok(Self {
                    mount,
                    file: whole
                        .canonicalize()
                        .with_context(|| format!("failed to resolve path {}", whole.display()))?,
                });
            }
            // Otherwise treat it as NAME:FILE.
            let name_part = &arg[..colon];
            let mount = parse_mount_name(name_part, arg)?;
            let file = Path::new(&arg[colon + 1..]);
            if !file.is_file() {
                anyhow::bail!("archive file not found: {}", file.display());
            }
            Ok(Self {
                mount,
                file: file
                    .canonicalize()
                    .with_context(|| format!("failed to resolve path {}", file.display()))?,
            })
        } else {
            let file = Path::new(arg);
            if !file.is_file() {
                anyhow::bail!("archive file not found: {}", file.display());
            }
            let name = stem(arg);
            let mount = parse_mount_name(&name, arg)?;
            Ok(Self {
                mount,
                file: file
                    .canonicalize()
                    .with_context(|| format!("failed to resolve path {}", file.display()))?,
            })
        }
    }
}

/// Parsed `--dynamic-empty` specification: either a plain name or a fixed path.
pub struct EmptySpec {
    pub mount: MountName,
}

impl EmptySpec {
    /// Parse a `NAME` or `/run/fuselage/NAME` argument.
    pub fn parse(arg: &str) -> Result<Self> {
        let mount = parse_mount_name(arg, arg)?;
        Ok(Self { mount })
    }
}

/// Parse a name-or-path string into a `MountName`.
///
/// Accepts either a plain relative name (validated by `validate_name`) or an
/// absolute path of the form `/run/fuselage/<single-component>`. Any other
/// absolute path is rejected with a clear error.
pub fn parse_mount_name(name: &str, source: &str) -> Result<MountName> {
    if let Some(rest) = name.strip_prefix("/run/fuselage/") {
        // Must be exactly one further component — no slashes, no empty tail.
        if rest.is_empty() || rest.contains('/') || rest.contains('\0') {
            anyhow::bail!(
                "fixed mount path {:?} must have exactly one component after /run/fuselage/ \
                 (e.g. /run/fuselage/myapp)",
                name
            );
        }
        if rest == "." || rest == ".." {
            anyhow::bail!("fixed mount path {:?} is not a valid directory name", name);
        }
        Ok(MountName::Fixed(PathBuf::from(name)))
    } else if name.starts_with('/') {
        anyhow::bail!(
            "absolute mount path {:?} (from {:?}) is not allowed; \
             only /run/fuselage/NAME paths are permitted",
            name,
            source
        );
    } else {
        validate_name(name, source)?;
        Ok(MountName::Relative(name.to_string()))
    }
}

/// Validate that a relative archive name is usable as a directory name.
///
/// A name must be non-empty, must not contain `/` or a null byte, and must not
/// be `.` or `..`. These are the minimal requirements for a safe directory name
/// component on Linux.
fn validate_name(name: &str, source: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!(
            "archive name derived from {:?} is empty; use NAME:FILE to provide an explicit name",
            source
        );
    }
    if name == "." || name == ".." {
        anyhow::bail!(
            "archive name {:?} (derived from {:?}) is not a valid directory name; \
             use NAME:FILE to provide an explicit name",
            name,
            source
        );
    }
    if name.contains('/') || name.contains('\0') {
        anyhow::bail!(
            "archive name {:?} (derived from {:?}) contains an invalid character; \
             use NAME:FILE to provide an explicit name",
            name,
            source
        );
    }
    Ok(())
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

/// Attempt to decode a base64-encoded archive (with optional leading `#` comment
/// lines) from `src` and write the decoded bytes to `dest`.
///
/// Returns `true` if the content looked like base64 and was successfully decoded,
/// `false` if the content clearly was not base64 (allowing the caller to produce a
/// better error message). Returns an error if the content appeared to be base64 but
/// decoding failed.
pub fn try_decode_base64(src: &Path, dest: &Path) -> Result<bool> {
    use std::io::{BufRead, BufReader};

    let f = fs::File::open(src).with_context(|| format!("failed to open {}", src.display()))?;
    let reader = BufReader::new(f);

    // Write to a temporary file alongside dest so that a partial decode never
    // leaves a corrupt file at the destination path.
    let tmp_dest = dest.with_extension("tmp");
    let mut dec: Option<crate::b64stream::B64Decoder<fs::File>> = None;

    let result = (|| {
        for line in reader.lines() {
            let line = line.with_context(|| format!("failed to read {}", src.display()))?;
            if line.starts_with('#') {
                // Skip comment lines — these carry herescript directives, not data.
                continue;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }

            // Open the destination file lazily so nothing is created for non-base64 inputs.
            if dec.is_none() {
                let out = fs::File::create(&tmp_dest)
                    .with_context(|| format!("failed to create {}", tmp_dest.display()))?;
                dec = Some(crate::b64stream::B64Decoder::new(out));
            }

            let accepted = dec
                .as_mut()
                .unwrap()
                .push_line(trimmed)
                .with_context(|| format!("base64 decode failed for {}", src.display()))?;

            if !accepted {
                return Ok(false);
            }
        }

        let Some(dec) = dec else {
            // No data lines were found.
            return Ok(false);
        };

        dec.finish()
            .with_context(|| format!("base64 decode failed for {}", src.display()))?;

        // Atomically move the completed file into place.
        fs::rename(&tmp_dest, dest).with_context(|| {
            format!(
                "failed to rename {} to {}",
                tmp_dest.display(),
                dest.display()
            )
        })?;

        Ok(true)
    })();

    // Clean up the temporary file on any error path after it was created.
    if result.is_err() {
        let _ = fs::remove_file(&tmp_dest);
    }

    result
}

/// Detect the archive format by reading the first 4 magic bytes.
pub fn detect_format(path: &Path) -> Result<ArchiveFormat> {
    let mut f =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)
        .with_context(|| format!("failed to read magic bytes from {}", path.display()))?;
    match &magic {
        b"PK\x03\x04" => Ok(ArchiveFormat::Zip),
        b"hsqs" | b"sqsh" => Ok(ArchiveFormat::Squashfs),
        b"\x7fELF" => detect_elf_squashfs(f, path),
        _ => anyhow::bail!(
            "{}: unrecognised archive format (magic {:02x?})",
            path.display(),
            magic
        ),
    }
}

/// Given a file whose first 4 bytes are ELF magic, locate the squashfs image
/// embedded immediately after the ELF's own on-disk data (rounded up to a
/// 4096-byte boundary) and return `ArchiveFormat::ElfSquashfs(offset)`.
///
/// The ELF's on-disk extent is the maximum of the section-header-table end, the
/// program-header-table end, and the highest PT_LOAD segment end. This matches
/// how fuselage-bundle appends the squashfs (after the whole compiled stub,
/// padded to a page boundary): using only the last PT_LOAD end would miss the
/// trailing section headers and symbol tables of a normal, non-stripped binary.
///
/// The offset is derived from ELF structure rather than a magic-byte scan, so
/// coincidental squashfs magic inside the ELF body cannot produce a false positive.
fn detect_elf_squashfs(mut f: fs::File, path: &Path) -> Result<ArchiveFormat> {
    // Read the remaining ELF identification bytes (e_ident is 16 bytes total;
    // we already consumed the first 4).
    let mut e_ident_rest = [0u8; 12];
    f.read_exact(&mut e_ident_rest)
        .with_context(|| format!("failed to read ELF ident from {}", path.display()))?;

    let class = e_ident_rest[0]; // EI_CLASS: 1 = 32-bit, 2 = 64-bit
    let data = e_ident_rest[1]; // EI_DATA:  1 = LE, 2 = BE

    // We only support the combinations that AppImages actually use (LE/BE × 32/64).
    let squashfs_offset = match (class, data) {
        (1, 1) => elf_squashfs_offset_32::<byteorder::LittleEndian>(&mut f, path),
        (1, 2) => elf_squashfs_offset_32::<byteorder::BigEndian>(&mut f, path),
        (2, 1) => elf_squashfs_offset_64::<byteorder::LittleEndian>(&mut f, path),
        (2, 2) => elf_squashfs_offset_64::<byteorder::BigEndian>(&mut f, path),
        _ => anyhow::bail!(
            "{}: ELF file has unsupported class/data encoding ({}/{})",
            path.display(),
            class,
            data
        ),
    }?;

    // Verify squashfs magic at the computed offset.
    f.seek(SeekFrom::Start(squashfs_offset))
        .with_context(|| format!("failed to seek in {}", path.display()))?;
    let mut sfs_magic = [0u8; 4];
    f.read_exact(&mut sfs_magic).with_context(|| {
        format!(
            "{}: ELF file has no squashfs image at expected offset {}",
            path.display(),
            squashfs_offset
        )
    })?;
    if &sfs_magic != b"hsqs" && &sfs_magic != b"sqsh" {
        anyhow::bail!(
            "{}: ELF file has no squashfs image at expected offset {} (found {:02x?})",
            path.display(),
            squashfs_offset,
            sfs_magic
        );
    }

    Ok(ArchiveFormat::ElfSquashfs(squashfs_offset))
}

/// Compute the squashfs offset for a 32-bit ELF: the 4096-aligned end of the
/// ELF's on-disk data (see `detect_elf_squashfs`).
fn elf_squashfs_offset_32<B: byteorder::ByteOrder>(f: &mut fs::File, path: &Path) -> Result<u64> {
    use byteorder::ReadBytesExt;

    // e_ident (16) already consumed. Read the rest of the 52-byte ELF32 header.
    // Fields: e_type(2), e_machine(2), e_version(4), e_entry(4), e_phoff(4),
    //         e_shoff(4), e_flags(4), e_ehsize(2), e_phentsize(2), e_phnum(2),
    //         e_shentsize(2), e_shnum(2), e_shstrndx(2)  → 36 bytes remaining.
    let _e_type = f.read_u16::<B>()?;
    let _e_machine = f.read_u16::<B>()?;
    let _e_version = f.read_u32::<B>()?;
    let _e_entry = f.read_u32::<B>()?;
    let e_phoff = f.read_u32::<B>()? as u64;
    let e_shoff = f.read_u32::<B>()? as u64;
    let _e_flags = f.read_u32::<B>()?;
    let _e_ehsize = f.read_u16::<B>()?;
    let e_phentsize = f.read_u16::<B>()? as u64;
    let e_phnum = f.read_u16::<B>()? as u64;
    let e_shentsize = f.read_u16::<B>()? as u64;
    let e_shnum = f.read_u16::<B>()? as u64;

    let load_end = highest_pt_load_end_32::<B>(f, path, e_phoff, e_phentsize, e_phnum)?;
    Ok(elf_image_offset(
        load_end,
        e_phoff,
        e_phentsize,
        e_phnum,
        e_shoff,
        e_shentsize,
        e_shnum,
    ))
}

/// Compute the squashfs offset for a 64-bit ELF: the 4096-aligned end of the
/// ELF's on-disk data (see `detect_elf_squashfs`).
fn elf_squashfs_offset_64<B: byteorder::ByteOrder>(f: &mut fs::File, path: &Path) -> Result<u64> {
    use byteorder::ReadBytesExt;

    // e_ident (16) already consumed. Read the rest of the 64-byte ELF64 header.
    // Fields: e_type(2), e_machine(2), e_version(4), e_entry(8), e_phoff(8),
    //         e_shoff(8), e_flags(4), e_ehsize(2), e_phentsize(2), e_phnum(2),
    //         e_shentsize(2), e_shnum(2), e_shstrndx(2)  → 48 bytes remaining.
    let _e_type = f.read_u16::<B>()?;
    let _e_machine = f.read_u16::<B>()?;
    let _e_version = f.read_u32::<B>()?;
    let _e_entry = f.read_u64::<B>()?;
    let e_phoff = f.read_u64::<B>()?;
    let e_shoff = f.read_u64::<B>()?;
    let _e_flags = f.read_u32::<B>()?;
    let _e_ehsize = f.read_u16::<B>()?;
    let e_phentsize = f.read_u16::<B>()? as u64;
    let e_phnum = f.read_u16::<B>()? as u64;
    let e_shentsize = f.read_u16::<B>()? as u64;
    let e_shnum = f.read_u16::<B>()? as u64;

    let load_end = highest_pt_load_end_64::<B>(f, path, e_phoff, e_phentsize, e_phnum)?;
    Ok(elf_image_offset(
        load_end,
        e_phoff,
        e_phentsize,
        e_phnum,
        e_shoff,
        e_shentsize,
        e_shnum,
    ))
}

/// Combine the program- and section-header table extents with the highest
/// PT_LOAD end to find the ELF's on-disk size, then round up to a 4096-byte
/// boundary. Saturating arithmetic is used because the header fields are
/// untrusted input: a corrupt or hostile ELF could otherwise overflow, and the
/// squashfs magic check at the resulting offset rejects any bogus value cleanly.
#[allow(clippy::too_many_arguments)]
fn elf_image_offset(
    load_end: u64,
    e_phoff: u64,
    e_phentsize: u64,
    e_phnum: u64,
    e_shoff: u64,
    e_shentsize: u64,
    e_shnum: u64,
) -> u64 {
    let pht_end = e_phoff.saturating_add(e_phnum.saturating_mul(e_phentsize));
    let sht_end = e_shoff.saturating_add(e_shnum.saturating_mul(e_shentsize));
    let image_end = load_end.max(pht_end).max(sht_end);
    align_up(image_end, 4096)
}

/// Scan 32-bit program headers and return the (unaligned) end of the last PT_LOAD segment.
fn highest_pt_load_end_32<B: byteorder::ByteOrder>(
    f: &mut fs::File,
    path: &Path,
    e_phoff: u64,
    e_phentsize: u64,
    e_phnum: u64,
) -> Result<u64> {
    use byteorder::ReadBytesExt;

    const PT_LOAD: u32 = 1;
    let mut highest_end: u64 = 0;

    for i in 0..e_phnum {
        let entry_offset = e_phoff + i * e_phentsize;
        f.seek(SeekFrom::Start(entry_offset)).with_context(|| {
            format!(
                "failed to seek to program header {} in {}",
                i,
                path.display()
            )
        })?;

        // ELF32 Phdr: p_type(4), p_offset(4), p_vaddr(4), p_paddr(4),
        //             p_filesz(4), p_memsz(4), p_flags(4), p_align(4)
        let p_type = f.read_u32::<B>()?;
        let p_offset = f.read_u32::<B>()? as u64;
        let _p_vaddr = f.read_u32::<B>()?;
        let _p_paddr = f.read_u32::<B>()?;
        let p_filesz = f.read_u32::<B>()? as u64;

        if p_type == PT_LOAD {
            let end = p_offset.saturating_add(p_filesz);
            if end > highest_end {
                highest_end = end;
            }
        }
    }

    anyhow::ensure!(
        highest_end > 0,
        "{}: ELF file has no PT_LOAD segments",
        path.display()
    );

    Ok(highest_end)
}

/// Scan 64-bit program headers and return the (unaligned) end of the last PT_LOAD segment.
fn highest_pt_load_end_64<B: byteorder::ByteOrder>(
    f: &mut fs::File,
    path: &Path,
    e_phoff: u64,
    e_phentsize: u64,
    e_phnum: u64,
) -> Result<u64> {
    use byteorder::ReadBytesExt;

    const PT_LOAD: u32 = 1;
    let mut highest_end: u64 = 0;

    for i in 0..e_phnum {
        let entry_offset = e_phoff + i * e_phentsize;
        f.seek(SeekFrom::Start(entry_offset)).with_context(|| {
            format!(
                "failed to seek to program header {} in {}",
                i,
                path.display()
            )
        })?;

        // ELF64 Phdr: p_type(4), p_flags(4), p_offset(8), p_vaddr(8), p_paddr(8),
        //             p_filesz(8), p_memsz(8), p_align(8)
        let p_type = f.read_u32::<B>()?;
        let _p_flags = f.read_u32::<B>()?;
        let p_offset = f.read_u64::<B>()?;
        let _p_vaddr = f.read_u64::<B>()?;
        let _p_paddr = f.read_u64::<B>()?;
        let p_filesz = f.read_u64::<B>()?;

        if p_type == PT_LOAD {
            let end = p_offset.saturating_add(p_filesz);
            if end > highest_end {
                highest_end = end;
            }
        }
    }

    anyhow::ensure!(
        highest_end > 0,
        "{}: ELF file has no PT_LOAD segments",
        path.display()
    );

    Ok(highest_end)
}

/// Round `value` up to the nearest multiple of `align` (which must be a power of two).
fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

/// Compute the SHA-256 hash of a file and return the first 16 hex characters.
pub fn compute_sha256(path: &Path) -> Result<String> {
    let mut f =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
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
    let file =
        fs::File::open(archive).with_context(|| format!("failed to open {}", archive.display()))?;
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
/// `offset` is the byte offset within `sfs` at which the squashfs image begins;
/// pass `0` for a standalone squashfs file.
///
/// Creates the directory hierarchy, regular files (with permissions), and
/// symlinks. Device nodes, named pipes, and sockets are skipped.
pub fn extract_squashfs(sfs: &Path, dest: &Path, offset: u64) -> Result<()> {
    use backhand::{FilesystemReader, InnerNode};

    let file = BufReader::new(
        fs::File::open(sfs).with_context(|| format!("failed to open {}", sfs.display()))?,
    );
    let filesystem = FilesystemReader::from_reader_with_offset(file, offset)
        .with_context(|| format!("failed to read squashfs image {}", sfs.display()))?;

    for node in filesystem.files() {
        // Strip the leading "/" from the stored fullpath.
        let rel = node.fullpath.strip_prefix("/").unwrap_or(&node.fullpath);
        let out = dest.join(rel);

        match &node.inner {
            InnerNode::Dir(_) if out != dest => {
                fs::create_dir_all(&out)
                    .with_context(|| format!("failed to create dir {}", out.display()))?;
            }
            InnerNode::Dir(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── stem() ────────────────────────────────────────────────────────────────

    #[test]
    fn stem_no_extension() {
        assert_eq!(stem("archive"), "archive");
    }

    #[test]
    fn stem_zip() {
        assert_eq!(stem("archive.zip"), "archive");
    }

    #[test]
    fn stem_sfs() {
        assert_eq!(stem("archive.sfs"), "archive");
    }

    #[test]
    fn stem_b64() {
        assert_eq!(stem("archive.b64"), "archive");
    }

    #[test]
    fn stem_with_directory() {
        assert_eq!(stem("/some/path/archive.zip"), "archive");
    }

    #[test]
    fn stem_double_known_extension_zip_b64() {
        // .b64 is stripped first, then .zip.
        assert_eq!(stem("archive.zip.b64"), "archive.zip");
    }

    #[test]
    fn stem_double_known_extension_sfs_zip() {
        // .zip is stripped first, leaving .sfs which is then stripped.
        // strip_suffix applies only the last matching suffix in sequence.
        assert_eq!(stem("archive.sfs.zip"), "archive.sfs");
    }

    #[test]
    fn stem_extension_only() {
        // ".zip" — strip_suffix removes ".zip", leaving the empty string.
        // This is an edge case with no practical use, but the behaviour is defined.
        assert_eq!(stem(".zip"), "");
    }

    #[test]
    fn stem_unknown_extension() {
        assert_eq!(stem("archive.tar.gz"), "archive.tar.gz");
    }

    // ── detect_format() ───────────────────────────────────────────────────────

    fn write_tmp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f
    }

    #[test]
    fn detect_format_zip() {
        let f = write_tmp(b"PK\x03\x04extra bytes");
        assert_eq!(detect_format(f.path()).unwrap(), ArchiveFormat::Zip);
    }

    #[test]
    fn detect_format_squashfs_little_endian() {
        let f = write_tmp(b"hsqsextra");
        assert_eq!(detect_format(f.path()).unwrap(), ArchiveFormat::Squashfs);
    }

    #[test]
    fn detect_format_squashfs_big_endian() {
        let f = write_tmp(b"sqshextra");
        assert_eq!(detect_format(f.path()).unwrap(), ArchiveFormat::Squashfs);
    }

    #[test]
    fn detect_format_unknown_magic() {
        let f = write_tmp(b"\x00\x01\x02\x03");
        assert!(detect_format(f.path()).is_err());
    }

    #[test]
    fn detect_format_too_small() {
        let f = write_tmp(b"PK\x03"); // Only 3 bytes — read_exact fails.
        assert!(detect_format(f.path()).is_err());
    }

    #[test]
    fn detect_format_nonexistent() {
        assert!(detect_format(std::path::Path::new("/nonexistent/path.zip")).is_err());
    }

    // ── compute_sha256() ──────────────────────────────────────────────────────

    #[test]
    fn compute_sha256_empty_file() {
        let f = write_tmp(b"");
        // SHA-256 of empty input: e3b0c44298fc1c14...; first 16 hex chars = "e3b0c44298fc1c14".
        assert_eq!(compute_sha256(f.path()).unwrap(), "e3b0c44298fc1c14");
    }

    #[test]
    fn compute_sha256_known_content() {
        let f = write_tmp(b"hello");
        // SHA-256("hello"): 2cf24dba5fb0a30e...; first 16 hex chars = "2cf24dba5fb0a30e".
        assert_eq!(compute_sha256(f.path()).unwrap(), "2cf24dba5fb0a30e");
    }

    #[test]
    fn compute_sha256_returns_16_chars() {
        let f = write_tmp(b"any content");
        assert_eq!(compute_sha256(f.path()).unwrap().len(), 16);
    }

    #[test]
    fn compute_sha256_nonexistent() {
        assert!(compute_sha256(std::path::Path::new("/nonexistent")).is_err());
    }

    // ── try_decode_base64() ───────────────────────────────────────────────────

    #[test]
    fn decode_base64_pure_content() {
        // "hello" base64-encoded is "aGVsbG8="
        let src = write_tmp(b"aGVsbG8=");
        let dest = tempfile::NamedTempFile::new().unwrap();
        let ok = try_decode_base64(src.path(), dest.path()).unwrap();
        assert!(ok);
        assert_eq!(fs::read(dest.path()).unwrap(), b"hello");
    }

    #[test]
    fn decode_base64_with_hash_comments() {
        // Comment lines preceding base64 data must be stripped.
        let src = write_tmp(b"# This is a comment\n## Another comment\naGVsbG8=\n");
        let dest = tempfile::NamedTempFile::new().unwrap();
        let ok = try_decode_base64(src.path(), dest.path()).unwrap();
        assert!(ok);
        assert_eq!(fs::read(dest.path()).unwrap(), b"hello");
    }

    #[test]
    fn decode_base64_multiline_data() {
        // Base64 data split across multiple lines must be reassembled.
        let src = write_tmp(b"# comment\naGVs\nbG8=\n");
        let dest = tempfile::NamedTempFile::new().unwrap();
        let ok = try_decode_base64(src.path(), dest.path()).unwrap();
        assert!(ok);
        assert_eq!(fs::read(dest.path()).unwrap(), b"hello");
    }

    #[test]
    fn decode_base64_binary_content_is_not_base64() {
        // A file with binary magic bytes should be rejected without error.
        let src = write_tmp(b"PK\x03\x04");
        let dest = tempfile::NamedTempFile::new().unwrap();
        let ok = try_decode_base64(src.path(), dest.path()).unwrap();
        assert!(!ok);
    }

    #[test]
    fn decode_base64_only_comments_is_not_base64() {
        // A file with only comment lines has no data to decode.
        let src = write_tmp(b"# no data here\n# nothing\n");
        let dest = tempfile::NamedTempFile::new().unwrap();
        let ok = try_decode_base64(src.path(), dest.path()).unwrap();
        assert!(!ok);
    }

    // ── validate_name() ───────────────────────────────────────────────────────

    #[test]
    fn validate_name_accepts_normal_name() {
        assert!(validate_name("mydata", "mydata.zip").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("", ".zip").is_err());
    }

    #[test]
    fn validate_name_rejects_dot() {
        assert!(validate_name(".", "./file.zip").is_err());
    }

    #[test]
    fn validate_name_rejects_dotdot() {
        assert!(validate_name("..", "../file.zip").is_err());
    }

    #[test]
    fn validate_name_rejects_slash() {
        assert!(validate_name("foo/bar", "foo/bar.zip").is_err());
    }

    #[test]
    fn validate_name_rejects_null_byte() {
        assert!(validate_name("foo\0bar", "foo\0bar.zip").is_err());
    }

    // ── detect_format() — ELF+squashfs ────────────────────────────────────────

    /// Build a minimal 64-bit little-endian ELF with one PT_LOAD segment followed
    /// by a squashfs magic cookie at a 4096-byte-aligned offset.
    ///
    /// ELF64 header: 64 bytes.
    /// ELF64 Phdr:   56 bytes (one entry).
    /// Total ELF content: 64 + 56 = 120 bytes → aligns up to 4096.
    fn make_elf64_le_with_squashfs(sfs_magic: &[u8; 4]) -> Vec<u8> {
        let mut buf = vec![0u8; 4096 + 8];

        // e_ident
        buf[0..4].copy_from_slice(b"\x7fELF");
        buf[4] = 2; // ELFCLASS64
        buf[5] = 1; // ELFDATA2LSB
        buf[6] = 1; // EV_CURRENT
        // bytes 7-15: padding (zeroes)

        // e_type, e_machine, e_version  (2+2+4 = 8 bytes at offset 16)
        buf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        buf[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); // EM_X86_64
        buf[20..24].copy_from_slice(&1u32.to_le_bytes()); // EV_CURRENT

        // e_entry (8), e_phoff (8), e_shoff (8)
        buf[24..32].copy_from_slice(&0u64.to_le_bytes()); // e_entry
        buf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff: right after header
        buf[40..48].copy_from_slice(&0u64.to_le_bytes()); // e_shoff: no section headers

        // e_flags (4), e_ehsize (2), e_phentsize (2), e_phnum (2),
        // e_shentsize (2), e_shnum (2), e_shstrndx (2)
        buf[48..52].copy_from_slice(&0u32.to_le_bytes()); // e_flags
        buf[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        buf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        buf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
        buf[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
        buf[60..62].copy_from_slice(&0u16.to_le_bytes()); // e_shnum
        buf[62..64].copy_from_slice(&0u16.to_le_bytes()); // e_shstrndx

        // ELF64 Phdr at offset 64:
        // p_type(4), p_flags(4), p_offset(8), p_vaddr(8), p_paddr(8),
        // p_filesz(8), p_memsz(8), p_align(8)
        buf[64..68].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        buf[68..72].copy_from_slice(&5u32.to_le_bytes()); // PF_R|PF_X
        buf[72..80].copy_from_slice(&0u64.to_le_bytes()); // p_offset: starts at 0
        buf[80..88].copy_from_slice(&0u64.to_le_bytes()); // p_vaddr
        buf[88..96].copy_from_slice(&0u64.to_le_bytes()); // p_paddr
        buf[96..104].copy_from_slice(&120u64.to_le_bytes()); // p_filesz: 64+56 bytes
        buf[104..112].copy_from_slice(&120u64.to_le_bytes()); // p_memsz
        buf[112..120].copy_from_slice(&4096u64.to_le_bytes()); // p_align

        // Squashfs magic immediately after the aligned ELF end (offset 4096).
        buf[4096..4100].copy_from_slice(sfs_magic);

        buf
    }

    #[test]
    fn detect_format_elf64_le_squashfs_little_endian() {
        let bytes = make_elf64_le_with_squashfs(b"hsqs");
        let f = write_tmp(&bytes);
        match detect_format(f.path()).unwrap() {
            ArchiveFormat::ElfSquashfs(offset) => assert_eq!(offset, 4096),
            other => panic!("expected ElfSquashfs, got {other:?}"),
        }
    }

    #[test]
    fn detect_format_elf64_le_squashfs_big_endian() {
        let bytes = make_elf64_le_with_squashfs(b"sqsh");
        let f = write_tmp(&bytes);
        match detect_format(f.path()).unwrap() {
            ArchiveFormat::ElfSquashfs(offset) => assert_eq!(offset, 4096),
            other => panic!("expected ElfSquashfs, got {other:?}"),
        }
    }

    /// Build a 64-bit little-endian ELF whose section-header table lives *after*
    /// the last PT_LOAD segment, with the squashfs on the page after that table.
    ///
    /// This mirrors a real, non-stripped compiler output: the loadable segments
    /// end early but trailing section headers / symbol tables extend the file.
    /// The squashfs offset must therefore be driven by the section-header-table
    /// end, not the last PT_LOAD end — the case that the simpler fixture misses.
    ///
    /// Layout: PT_LOAD ends at 120; section-header table at offset 4097 with two
    /// 64-byte entries (end 4225); squashfs magic on the next page at 8192.
    fn make_elf64_le_with_trailing_sht(sfs_magic: &[u8; 4]) -> Vec<u8> {
        let mut buf = vec![0u8; 8192 + 8];

        buf[0..4].copy_from_slice(b"\x7fELF");
        buf[4] = 2; // ELFCLASS64
        buf[5] = 1; // ELFDATA2LSB
        buf[6] = 1; // EV_CURRENT

        buf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        buf[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); // EM_X86_64
        buf[20..24].copy_from_slice(&1u32.to_le_bytes()); // EV_CURRENT

        buf[24..32].copy_from_slice(&0u64.to_le_bytes()); // e_entry
        buf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff: right after header
        buf[40..48].copy_from_slice(&4097u64.to_le_bytes()); // e_shoff: past the last PT_LOAD

        buf[48..52].copy_from_slice(&0u32.to_le_bytes()); // e_flags
        buf[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        buf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        buf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
        buf[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
        buf[60..62].copy_from_slice(&2u16.to_le_bytes()); // e_shnum: sht_end = 4097 + 2*64 = 4225
        buf[62..64].copy_from_slice(&0u16.to_le_bytes()); // e_shstrndx

        // ELF64 Phdr at offset 64: one PT_LOAD covering bytes 0..120.
        buf[64..68].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        buf[68..72].copy_from_slice(&5u32.to_le_bytes()); // PF_R|PF_X
        buf[72..80].copy_from_slice(&0u64.to_le_bytes()); // p_offset
        buf[80..88].copy_from_slice(&0u64.to_le_bytes()); // p_vaddr
        buf[88..96].copy_from_slice(&0u64.to_le_bytes()); // p_paddr
        buf[96..104].copy_from_slice(&120u64.to_le_bytes()); // p_filesz
        buf[104..112].copy_from_slice(&120u64.to_le_bytes()); // p_memsz
        buf[112..120].copy_from_slice(&4096u64.to_le_bytes()); // p_align

        // Squashfs magic on the page after the section-header table (offset 8192).
        buf[8192..8196].copy_from_slice(sfs_magic);

        buf
    }

    #[test]
    fn detect_format_elf_offset_follows_section_header_table() {
        // The squashfs sits past the section-header table, on a different page
        // than the last PT_LOAD end (4096). The offset must be 8192, proving the
        // detector accounts for trailing section data rather than stopping at the
        // last loadable segment.
        let bytes = make_elf64_le_with_trailing_sht(b"hsqs");
        let f = write_tmp(&bytes);
        match detect_format(f.path()).unwrap() {
            ArchiveFormat::ElfSquashfs(offset) => assert_eq!(offset, 8192),
            other => panic!("expected ElfSquashfs, got {other:?}"),
        }
    }

    #[test]
    fn detect_format_elf_without_squashfs_is_error() {
        // ELF file with no squashfs appended — should return an error.
        let mut bytes = make_elf64_le_with_squashfs(b"hsqs");
        // Overwrite the squashfs magic with garbage.
        bytes[4096..4100].copy_from_slice(b"\x00\x00\x00\x00");
        let f = write_tmp(&bytes);
        assert!(
            detect_format(f.path()).is_err(),
            "ELF without squashfs should be an error"
        );
    }

    #[test]
    fn align_up_already_aligned() {
        assert_eq!(align_up(4096, 4096), 4096);
    }

    #[test]
    fn align_up_one_past() {
        assert_eq!(align_up(4097, 4096), 8192);
    }

    #[test]
    fn align_up_zero() {
        assert_eq!(align_up(0, 4096), 0);
    }
}
