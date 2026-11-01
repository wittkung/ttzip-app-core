// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Chunk-aligned `.ttpart` resumable streaming writer with coarse-grained fsync and atomic rename.
//!
//! Provides deterministic boundary truncation for partial trailing chunks upon resumption,
//! zero NVMe write-amplification per-chunk streaming, and single-step atomic finalization.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Standard file extension appended to destination files during active transfer streaming.
pub const PENDING_PART_EXTENSION: &str = "ttpart";

/// Chunk-aligned streaming writer targeting an intermediate `.ttpart` staging file.
///
/// On initialization, any existing partial file at `<dest_path>.ttpart` is inspected:
/// valid transferred bytes are calculated strictly on chunk alignment boundaries,
/// trailing corrupt/unaligned bytes are truncated via [`File::set_len`], and the write
/// cursor is positioned at the aligned boundary.
#[derive(Debug)]
pub struct PartWriter {
    dest_path: PathBuf,
    part_path: PathBuf,
    file: File,
    written_bytes: u64,
    chunk_alignment: usize,
}

impl PartWriter {
    /// Opens or resumes an aligned `.ttpart` staging file for the given destination path.
    ///
    /// # Errors
    /// Returns [`io::Error`] with [`io::ErrorKind::InvalidInput`] if `chunk_alignment` is 0
    /// or if `dest_path` does not contain a valid filename. Any filesystem permissions or
    /// missing parent directory errors are propagated directly.
    pub fn open(dest_path: impl AsRef<Path>, chunk_alignment: usize) -> io::Result<Self> {
        if chunk_alignment == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Chunk alignment must be greater than zero",
            ));
        }

        let dest_path = dest_path.as_ref().to_path_buf();
        let part_path = match dest_path.file_name() {
            Some(file_name) => {
                let mut staging_name = file_name.to_os_string();
                staging_name.push(".");
                staging_name.push(PENDING_PART_EXTENSION);
                dest_path.with_file_name(staging_name)
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Destination path must have a valid file name",
                ));
            }
        };

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part_path)?;

        let existing_len = file.metadata()?.len();
        let alignment_u64 = chunk_alignment as u64;
        let written_bytes = (existing_len / alignment_u64) * alignment_u64;

        if existing_len != written_bytes {
            file.set_len(written_bytes)?;
        }
        file.seek(SeekFrom::Start(written_bytes))?;

        Ok(Self {
            dest_path,
            part_path,
            file,
            written_bytes,
            chunk_alignment,
        })
    }

    /// Appends a raw chunk slice into the intermediate `.ttpart` staging file.
    ///
    /// This method performs sequential buffered write without calling `fsync` or `sync_data`
    /// per chunk, preventing flash wear and write amplification. Returns the updated total
    /// transferred bytes.
    #[inline]
    pub fn append_chunk(&mut self, chunk: &[u8]) -> io::Result<u64> {
        self.file.write_all(chunk)?;
        self.written_bytes = self.written_bytes.saturating_add(chunk.len() as u64);
        Ok(self.written_bytes)
    }

    /// Flushes dirty page cache buffers and commits file metadata to disk storage.
    ///
    /// Should be invoked on explicit coordinator checkpoints or graceful transfer pauses.
    #[inline]
    pub fn sync(&mut self) -> io::Result<()> {
        self.file.sync_data()
    }

    /// Flushes remaining buffers, releases the staging file handle, and atomically moves
    /// `<dest_path>.ttpart` to the final destination path.
    ///
    /// Returns the verified destination path upon successful atomic replacement.
    pub fn finalize(mut self) -> io::Result<PathBuf> {
        self.sync()?;
        drop(self.file);
        std::fs::rename(&self.part_path, &self.dest_path)?;
        Ok(self.dest_path)
    }

    /// Returns the current chunk-aligned committed byte count.
    #[inline]
    pub fn written_bytes(&self) -> u64 {
        self.written_bytes
    }

    /// Returns the reference to the intermediate `.ttpart` file path.
    #[inline]
    pub fn part_path(&self) -> &Path {
        &self.part_path
    }

    /// Returns the reference to the final target destination path.
    #[inline]
    pub fn dest_path(&self) -> &Path {
        &self.dest_path
    }

    /// Returns the configured chunk alignment boundary size in bytes.
    #[inline]
    pub fn chunk_alignment(&self) -> usize {
        self.chunk_alignment
    }
}

// Compile-time static assertions ensuring thread-safety and concurrency invariants.
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    const fn assert_unpin<T: Unpin>() {}

    assert_send::<PartWriter>();
    assert_sync::<PartWriter>();
    assert_unpin::<PartWriter>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;

    #[test]
    fn test_part_writer_new_file_and_append() {
        let dir = tempdir().expect("tempdir creation failed");
        let dest = dir.path().join("archive.tar");
        let alignment = 1024;

        let mut writer = PartWriter::open(&dest, alignment).expect("open failed");
        assert_eq!(writer.written_bytes(), 0);
        assert_eq!(writer.dest_path(), dest.as_path());
        assert_eq!(
            writer.part_path(),
            dir.path().join("archive.tar.ttpart").as_path()
        );
        assert_eq!(writer.chunk_alignment(), alignment);

        let chunk1 = vec![0xAA; 512];
        let chunk2 = vec![0xBB; 512];

        let offset1 = writer.append_chunk(&chunk1).expect("append chunk1 failed");
        assert_eq!(offset1, 512);
        assert_eq!(writer.written_bytes(), 512);

        let offset2 = writer.append_chunk(&chunk2).expect("append chunk2 failed");
        assert_eq!(offset2, 1024);
        assert_eq!(writer.written_bytes(), 1024);

        writer.sync().expect("sync failed");

        let finalized_path = writer.finalize().expect("finalize failed");
        assert_eq!(finalized_path, dest);
        assert!(dest.exists());
        assert!(!dir.path().join("archive.tar.ttpart").exists());

        let mut content = Vec::new();
        let mut f = File::open(&dest).expect("open final file failed");
        f.read_to_end(&mut content).expect("read content failed");
        assert_eq!(content.len(), 1024);
        assert_eq!(&content[..512], &chunk1[..]);
        assert_eq!(&content[512..], &chunk2[..]);
    }

    #[test]
    fn test_part_writer_resumption_and_partial_chunk_truncation() {
        let dir = tempdir().expect("tempdir creation failed");
        let dest = dir.path().join("payload.bin");
        let part = dir.path().join("payload.bin.ttpart");
        let alignment = 1000;

        // Simulate interrupted write: 2 full 1000-byte chunks + 345 corrupt trailing bytes
        let mut initial_bytes = Vec::with_capacity(2345);
        initial_bytes.extend(std::iter::repeat_n(0x11, 1000));
        initial_bytes.extend(std::iter::repeat_n(0x22, 1000));
        initial_bytes.extend(std::iter::repeat_n(0xFF, 345)); // corrupt trailing partial chunk
        std::fs::write(&part, &initial_bytes).expect("write seed part failed");

        // Open with chunk alignment = 1000: must truncate to 2000 bytes
        let mut writer = PartWriter::open(&dest, alignment).expect("open resumption failed");
        assert_eq!(writer.written_bytes(), 2000);

        // Verify physical file on disk was truncated immediately
        let meta = std::fs::metadata(&part).expect("metadata failed");
        assert_eq!(meta.len(), 2000);

        // Resume writing chunk 3 (1000 bytes)
        let chunk3 = vec![0x33; 1000];
        let offset3 = writer.append_chunk(&chunk3).expect("append chunk3 failed");
        assert_eq!(offset3, 3000);
        assert_eq!(writer.written_bytes(), 3000);

        let final_dest = writer.finalize().expect("finalize failed");
        assert_eq!(final_dest, dest);

        let mut read_back = Vec::new();
        let mut f = File::open(&dest).expect("open final file failed");
        f.read_to_end(&mut read_back).expect("read final content failed");
        assert_eq!(read_back.len(), 3000);
        assert_eq!(&read_back[..1000], &initial_bytes[..1000]);
        assert_eq!(&read_back[1000..2000], &initial_bytes[1000..2000]);
        assert_eq!(&read_back[2000..3000], &chunk3[..]);
    }

    #[test]
    fn test_part_writer_invalid_directory_error() {
        let invalid_dest = PathBuf::from("/nonexistent_dir_999888/subdir/out.bin");
        let result = PartWriter::open(&invalid_dest, 1024);
        assert!(result.is_err());
    }

    #[test]
    fn test_part_writer_zero_alignment_error() {
        let dir = tempdir().expect("tempdir creation failed");
        let dest = dir.path().join("zero.bin");
        let result = PartWriter::open(&dest, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_part_writer_empty_path_error() {
        let result = PartWriter::open(Path::new(""), 1024);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
