use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use actix_web::http::header::ContentEncoding;
use libflate::gzip::Encoder;
use serde::Deserialize;
use streaming_zip;
use strum::{Display, EnumIter, EnumString};
use tar::Builder;

use crate::errors::ContextualError;

/// Available archive methods
#[derive(Deserialize, Clone, Copy, EnumIter, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ArchiveMethod {
    /// Gzipped tarball
    TarGz,

    /// Regular tarball
    Tar,

    /// Regular zip
    Zip,
}

impl ArchiveMethod {
    pub fn extension(self) -> String {
        match self {
            ArchiveMethod::TarGz => "tar.gz",
            ArchiveMethod::Tar => "tar",
            ArchiveMethod::Zip => "zip",
        }
        .to_string()
    }

    pub fn content_type(self) -> String {
        match self {
            ArchiveMethod::TarGz => "application/gzip",
            ArchiveMethod::Tar => "application/tar",
            ArchiveMethod::Zip => "application/zip",
        }
        .to_string()
    }

    pub fn content_encoding(self) -> ContentEncoding {
        match self {
            ArchiveMethod::TarGz => ContentEncoding::Gzip,
            ArchiveMethod::Tar => ContentEncoding::Identity,
            ArchiveMethod::Zip => ContentEncoding::Identity,
        }
    }

    pub fn is_enabled(self, tar_enabled: bool, tar_gz_enabled: bool, zip_enabled: bool) -> bool {
        match self {
            ArchiveMethod::TarGz => tar_gz_enabled,
            ArchiveMethod::Tar => tar_enabled,
            ArchiveMethod::Zip => zip_enabled,
        }
    }

    /// Make an archive out of the given directory, and write the output to the given writer.
    ///
    /// Recursively includes all files and subdirectories.
    ///
    /// If `skip_symlinks` is `true`, symlinks fill not be followed and will just be ignored.
    pub fn create_archive<T, W>(
        self,
        dir: T,
        skip_symlinks: bool,
        out: W,
    ) -> Result<(), ContextualError>
    where
        T: AsRef<Path>,
        W: std::io::Write,
    {
        let dir = dir.as_ref();
        match self {
            ArchiveMethod::TarGz => tar_gz(dir, skip_symlinks, out),
            ArchiveMethod::Tar => tar_dir(dir, skip_symlinks, out),
            ArchiveMethod::Zip => zip_dir(dir, skip_symlinks, out),
        }
    }
}

/// Write a gzipped tarball of `dir` in `out`.
fn tar_gz<W>(dir: &Path, skip_symlinks: bool, out: W) -> Result<(), ContextualError>
where
    W: std::io::Write,
{
    let mut out = Encoder::new(out).map_err(|e| ContextualError::IoError("GZIP".to_string(), e))?;

    tar_dir(dir, skip_symlinks, &mut out)?;

    out.finish()
        .into_result()
        .map_err(|e| ContextualError::IoError("GZIP finish".to_string(), e))?;

    Ok(())
}

/// Write a tarball of `dir` in `out`.
///
/// The target directory will be saved as a top-level directory in the archive.
///
/// For example, consider this directory structure:
///
/// ```ignore
/// a
/// └── b
///     └── c
///         ├── e
///         ├── f
///         └── g
/// ```
///
/// Making a tarball out of `"a/b/c"` will result in this archive content:
///
/// ```ignore
/// c
/// ├── e
/// ├── f
/// └── g
/// ```
fn tar_dir<W>(dir: &Path, skip_symlinks: bool, out: W) -> Result<(), ContextualError>
where
    W: std::io::Write,
{
    let inner_folder = dir.file_name().ok_or_else(|| {
        ContextualError::InvalidPathError("Directory name terminates in \"..\"".to_string())
    })?;

    let directory = inner_folder.to_str().ok_or_else(|| {
        ContextualError::InvalidPathError(
            "Directory name contains invalid UTF-8 characters".to_string(),
        )
    })?;

    tar(dir, directory.to_string(), skip_symlinks, out)
        .map_err(|e| ContextualError::ArchiveCreationError("tarball".to_string(), Box::new(e)))
}

/// Writes a tarball of `dir` in `out`.
///
/// The content of `src_dir` will be saved in the archive as a folder named `inner_folder`.
fn tar<W>(
    src_dir: &Path,
    inner_folder: String,
    skip_symlinks: bool,
    out: W,
) -> Result<(), ContextualError>
where
    W: std::io::Write,
{
    let mut tar_builder = Builder::new(out);

    tar_builder.follow_symlinks(!skip_symlinks);

    // Recursively adds the content of src_dir into the archive stream
    tar_builder
        .append_dir_all(inner_folder, src_dir)
        .map_err(|e| {
            ContextualError::IoError(
                format!(
                    "Failed to append the content of {} to the TAR archive",
                    src_dir.to_str().unwrap_or("file")
                ),
                e,
            )
        })?;

    // Finish the archive
    tar_builder.into_inner().map_err(|e| {
        ContextualError::IoError("Failed to finish writing the TAR archive".to_string(), e)
    })?;

    Ok(())
}

/// Write a zip of `dir` in `out`.
///
/// The target directory will be saved as a top-level directory in the archive.
///
/// For example, consider this directory structure:
///
/// ```ignore
/// a
/// └── b
///     └── c
///         ├── e
///         ├── f
///         └── g
/// ```
///
/// Making a zip out of `"a/b/c"` will result in this archive content:
///
/// ```ignore
/// c
/// ├── e
/// ├── f
/// └── g
/// ```
fn zip_dir<W: Write>(dir: &Path, skip_symlinks: bool, out: W) -> Result<(), ContextualError> {
    // TODO: implement skip_symlinks (I don't know the current behaviour)
    let mut zip_writer = streaming_zip::Archive::new(out);

    let dir_name = dir
        .file_name()
        .ok_or(ContextualError::InvalidPathError(format!(
            "Could not get the directory name of {:?}",
            dir
        )))?;
    let dir_path = dir
        .to_str()
        .ok_or(ContextualError::InvalidPathError(format!(
            "Could not get the path of {:?}",
            dir
        )))?;
    zip_writer
        .add_dir_all(
            dir_name,
            dir,
            streaming_zip::CompressionMode::Deflate(3),
            false,
        )
        .map_err(|e| {
            ContextualError::ArchiveCreationError(
                "ZIP".to_string(),
                Box::new(ContextualError::IoError(
                    format!(
                        "Failed to append the content of {} to the ZIP archive",
                        dir_path
                    ),
                    e,
                )),
            )
        })?;
    zip_writer.finish().map_err(|e| {
        ContextualError::ArchiveCreationError(
            "ZIP finish".to_string(),
            Box::new(ContextualError::IoError(
                "Failed to finish writing to the ZIP archive".to_string(),
                e,
            )),
        )
    })?;

    Ok(())
}
