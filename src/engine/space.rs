//! Room on the disk, looked at before a download starts: better to say so at once than to fail
//! with "disk full" after an hour, or at 4 GB on a USB stick formatted FAT32.

use std::path::Path;

/// FAT32 can't hold a file of 4 GiB or more.
const FAT_LARGEST: u64 = (4 << 30) - 1;

/// Why `bytes` more can't be written into a file in `dir`, if they can't.
pub fn shortage(dir: &Path, bytes: u64, file_size: Option<u64>) -> Option<String> {
    let dir = dir.ancestors().find(|d| d.exists())?;
    let (free, fat) = volume(dir)?;
    if fat && file_size.is_some_and(|size| size > FAT_LARGEST) {
        return Some("The file is over 4 GB, too big for this disk (it's formatted FAT32): \
                     choose another folder, or reformat the disk as exFAT or NTFS".into());
    }
    (bytes > free).then(|| {
        let size = |n| crate::util::human_size(Some(n));
        format!("Not enough disk space: {} more is needed, and {} is free on that disk", size(bytes), size(free))
    })
}

/// Free bytes (for this user) on the disk of `dir`, and whether it's FAT.
#[cfg(windows)]
fn volume(dir: &Path) -> Option<(u64, bool)> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW};
    let path = HSTRING::from(dir.as_os_str());
    let mut free = 0u64;
    // SAFETY: plain queries into our own number and buffers
    unsafe { GetDiskFreeSpaceExW(&path, Some(&mut free), None, None) }.ok()?;
    let (mut root, mut fs) = ([0u16; 261], [0u16; 32]);
    let fat = unsafe { GetVolumePathNameW(&path, &mut root) }.is_ok()
        && unsafe { GetVolumeInformationW(windows::core::PCWSTR(root.as_ptr()), None, None, None, None, Some(&mut fs)) }.is_ok()
        && String::from_utf16_lossy(&fs).trim_end_matches('\0').eq_ignore_ascii_case("FAT32");
    Some((free, fat))
}

#[cfg(unix)]
fn volume(dir: &Path) -> Option<(u64, bool)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: a valid C string and a zeroed struct for statvfs to fill
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)] // (the fields' types differ between systems)
    let free = st.f_bavail as u64 * st.f_frsize as u64;
    #[cfg(target_os = "linux")]
    let fat = {
        const MSDOS_SUPER_MAGIC: i64 = 0x4d44;
        // SAFETY: as above
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        #[allow(clippy::unnecessary_cast)]
        let kind = (unsafe { libc::statfs(c.as_ptr(), &mut fs) } == 0).then_some(fs.f_type as i64);
        kind == Some(MSDOS_SUPER_MAGIC)
    };
    #[cfg(not(target_os = "linux"))]
    let fat = false;
    Some((free, fat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_the_disk() {
        let dir = std::env::temp_dir();
        assert!(volume(&dir).is_some_and(|(free, _)| free > 0));
        assert_eq!(shortage(&dir, 1, Some(1)), None);
        assert!(shortage(&dir, u64::MAX / 2, None).unwrap().contains("Not enough disk space"));
        // (a folder not made yet: its nearest existing parent's disk)
        assert_eq!(shortage(&dir.join("not/made/yet"), 1, None), None);
    }
}
