//! A file on disk as a [`ByteSource`].
//!
//! The one source here that names tokio, and the reason this module is behind
//! `assets-fs`: a read from a file blocks the thread it runs on, so every one
//! runs on tokio's blocking pool.

use std::{fs::File, io, sync::Arc};

use bytes::{Bytes, BytesMut};

use super::{ByteSource, SPAN};

/// An open file, read by position.
///
/// Built from a handle rather than a path, so the octets served are those of
/// the file that was opened and stated, even when another one is renamed over
/// its path mid-response. Every read names its own offset (`pread` on Unix,
/// `ReadFile` with an offset on Windows), so concurrent spans need no lock and
/// no seek, and the handle's own cursor is never consulted.
///
/// The length is the one [`metadata`](Self::metadata) holds, taken once by
/// [`new`](Self::new), so it always agrees with the validators an application
/// derives from that metadata. A file that shrinks afterwards fails the body
/// with [`Truncated`](super::Truncated); one that grows is served at the length
/// it was opened with.
///
/// ```no_run
/// use kynos::{
///     http::media::MediaType,
///     response::range::{
///         served::{Conditions, Delivery, Served},
///         source::file::FileSource,
///     },
/// };
///
/// struct Mpeg;
///
/// impl MediaType for Mpeg {
///     const MEDIA_TYPE: &'static str = "audio/mpeg";
/// }
///
/// async fn recording(conditions: Conditions) -> std::io::Result<Delivery<Mpeg>> {
///     let source = FileSource::new(std::fs::File::open("recording.mp3")?).await?;
///     let modified = source.metadata().modified()?;
///
///     Served::<_, Mpeg>::new(source)
///         .last_modified(modified)
///         .deliver(&conditions)
///         .await
/// }
/// ```
///
/// A [`tokio::fs::File`] becomes one through
/// [`into_std`](tokio::fs::File::into_std).
#[derive(Debug)]
pub struct FileSource {
    /// Shared with the blocking task each read runs on, which must own what it
    /// reads from.
    file: Arc<File>,
    metadata: std::fs::Metadata,
}

impl FileSource {
    /// Takes ownership of an open file and reads its metadata from the handle.
    ///
    /// # Errors
    ///
    /// Where the handle cannot be stated, and with
    /// [`InvalidInput`](io::ErrorKind::InvalidInput) where it is not a regular
    /// file: a directory, a pipe or a device has no length to serve a range
    /// of.
    pub async fn new(file: File) -> io::Result<Self> {
        let file = Arc::new(file);
        let metadata = blocking({
            let file = Arc::clone(&file);
            move || file.metadata()
        })
        .await?;

        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a file source reads a regular file, and this handle is not one",
            ));
        }

        Ok(Self { file, metadata })
    }

    /// What the handle was stated as when the source was built.
    ///
    /// For the validators: an `ETag` from its length and modification time, a
    /// `Last-Modified` from [`modified`](std::fs::Metadata::modified). Which
    /// scheme the application mints is its own.
    #[must_use]
    pub const fn metadata(&self) -> &std::fs::Metadata {
        &self.metadata
    }
}

impl ByteSource for FileSource {
    type Error = io::Error;

    async fn complete_length(&self) -> Result<u64, Self::Error> {
        Ok(self.metadata.len())
    }

    /// Reads one [`SPAN`] at most, so a caller asking for the whole file
    /// directly is answered short rather than buffering all of it.
    async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
        let length = usize::try_from(last.saturating_sub(first).saturating_add(1).min(SPAN))
            .unwrap_or(usize::MAX);
        let file = Arc::clone(&self.file);

        blocking(move || {
            let mut buffer = BytesMut::zeroed(length);
            let read = read_at(&file, &mut buffer, first)?;
            buffer.truncate(read);
            Ok(buffer.freeze())
        })
        .await
    }
}

/// Runs `work` on the blocking pool.
///
/// The task ends in error only where it panicked or the runtime shut down
/// under it, and both are answered as an I/O error rather than unwound into
/// the body.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|stopped| Err(io::Error::other(stopped)))
}

/// One positional read into `buffer` from `offset`, retried where a signal
/// interrupted it.
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    loop {
        match positional(file, buffer, offset) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            read => return read,
        }
    }
}

#[cfg(unix)]
fn positional(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

/// Moves the handle's cursor as well, which nothing here reads.
#[cfg(windows)]
fn positional(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

/// A target with neither has no positional read in `std`.
#[cfg(not(any(unix, windows)))]
fn positional(_: &File, _: &mut [u8], _: u64) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this target has no positional file read",
    ))
}

#[cfg(test)]
mod tests;
