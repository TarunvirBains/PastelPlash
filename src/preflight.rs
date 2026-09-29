//! Checks before a run: the output's estimated size against the free space where it goes, so a
//! run that enlarges textures (resolution floors) fails at once instead of hours in.

use std::path::Path;

use anyhow::{Result, bail};

/// Extra room kept free beyond the estimate (archive directory, file system slack).
const MARGIN: u64 = 256 << 20;

/// What a run will write, estimated before it starts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    /// Bytes of output.
    pub bytes: u64,
    /// Files written larger than their source (resolution floors).
    pub enlarged: usize,
    /// The largest image the pipeline will hold, in texels (after enlargement, at the internal
    /// size). Images above the GPU chunk size (`PASTELPLASH_MAX_CHUNK` or the device's limit)
    /// run in chunks, so VRAM stays bounded.
    pub max_texels: u64,
}

impl Estimate {
    pub fn add(&mut self, bytes: u64, factor: u32, texels: u64) {
        self.bytes += bytes;
        if factor > 1 {
            self.enlarged += 1;
        }
        self.max_texels = self.max_texels.max(texels);
    }

    pub fn summary(&self) -> String {
        format!(
            "estimated output {:.1} GB ({} enlarged for the resolution floor; largest image {:.1} Mtexels)",
            gb(self.bytes),
            self.enlarged,
            self.max_texels as f64 / 1e6
        )
    }
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}

/// Fails with a clear message if the volume holding `output` (a file or folder, created or not)
/// has less free space than `needed` plus a margin. Unknown free space only warns.
pub fn check_disk(output: &Path, needed: u64) -> Result<()> {
    let Some(dir) = output.ancestors().find(|p| p.is_dir()) else {
        return Ok(());
    };
    match free_space(dir) {
        Some(free) if free < needed + MARGIN => bail!(
            "not enough disk space for {}: the output needs about {:.1} GB, {:.1} GB free on its drive",
            output.display(),
            gb(needed + MARGIN),
            gb(free)
        ),
        Some(_) => Ok(()),
        None => {
            eprintln!(
                "warning: could not read the free space for {}; not checked",
                dir.display()
            );
            Ok(())
        }
    }
}

/// Free bytes available to this user on the volume holding `dir`.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            dir: *const u16,
            avail: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let mut wide: Vec<u16> = dir.as_os_str().encode_wide().collect();
    if wide.last() != Some(&u16::from(b'\\')) {
        wide.push(u16::from(b'\\'));
    }
    wide.push(0);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: a NUL-terminated wide path and three valid out-pointers.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some(avail)
}

/// Free bytes available on the volume holding `dir` (not measured on this platform).
#[cfg(not(windows))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_impossible_output_fails_before_the_run() {
        let dir = std::env::temp_dir();
        if free_space(&dir).is_none() {
            return; // not measurable here
        }
        let err = check_disk(&dir.join("out.o2r"), u64::MAX / 4).unwrap_err();
        assert!(err.to_string().contains("not enough disk space"), "{err}");
        check_disk(&dir.join("out.o2r"), 1).unwrap();
    }

    #[test]
    fn estimates_add_up() {
        let mut e = Estimate::default();
        e.add(100, 1, 64);
        e.add(1600, 4, 1024);
        assert_eq!(
            e,
            Estimate {
                bytes: 1700,
                enlarged: 1,
                max_texels: 1024
            }
        );
    }
}
