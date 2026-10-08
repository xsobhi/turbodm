//! Writing downloaded data: collected into large blocks per connection and written on the
//! blocking-I/O threads, so a busy disk never stops the connections from reading. At gigabit
//! speeds a write per network chunk (~16 KiB) would stall them all whenever the disk lags.

use std::fs::File;
use std::sync::Arc;

/// Write this much at once.
pub const BLOCK: usize = 1 << 20;

/// Bytes received for one segment, not written yet: contiguous, from `offset`.
pub struct Pending {
    offset: u64,
    data: Vec<u8>,
}

impl Pending {
    pub fn new() -> Self {
        Pending { offset: 0, data: Vec::with_capacity(BLOCK) }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Add bytes that belong at `offset` (right after what's already here).
    pub fn push(&mut self, offset: u64, bytes: &[u8]) {
        if self.data.is_empty() {
            self.offset = offset;
        }
        debug_assert_eq!(self.offset + self.data.len() as u64, offset);
        self.data.extend_from_slice(bytes);
    }

    /// Write everything to the file; returns how many bytes that was.
    pub async fn flush(&mut self, file: &Arc<File>) -> Result<usize, String> {
        if self.data.is_empty() {
            return Ok(0);
        }
        let (file, data, offset) = (file.clone(), std::mem::take(&mut self.data), self.offset);
        let (result, mut data) = tokio::task::spawn_blocking(move || (write_all_at(&file, &data, offset), data))
            .await.map_err(|e| format!("Cannot write the file: {e}"))?;
        result.map_err(|e| format!("Cannot write the file: {e}"))?;
        let written = data.len();
        data.clear();
        self.data = data; // keep the buffer for the next block
        Ok(written)
    }
}

/// Write `buf` at `offset` without moving a shared cursor: every connection writes into the
/// same open file.
#[cfg(unix)]
fn write_all_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset)? {
            0 => return Err(std::io::ErrorKind::WriteZero.into()),
            n => (buf, offset) = (&buf[n..], offset + n as u64),
        }
    }
    Ok(())
}

/// Windows: make a new file sparse. Otherwise NTFS fills everything before a write far into
/// the file with zeros first, and the download waits for it (seconds, on big files).
#[cfg(windows)]
pub fn make_sparse(file: &File) {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    unsafe extern "system" {
        fn DeviceIoControl(device: *mut c_void, code: u32, input: *const c_void, input_size: u32,
                           output: *mut c_void, output_size: u32, returned: *mut u32, overlapped: *mut c_void) -> i32;
    }
    const FSCTL_SET_SPARSE: u32 = 0x0009_00C4;
    let mut returned = 0;
    // SAFETY: a valid open file handle; no buffers; failing (e.g. FAT32) just leaves it as it is
    unsafe {
        DeviceIoControl(file.as_raw_handle().cast(), FSCTL_SET_SPARSE, std::ptr::null(), 0,
                        std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut());
    }
}

/// Other systems create files with holes already.
#[cfg(not(windows))]
pub fn make_sparse(_file: &File) {}
