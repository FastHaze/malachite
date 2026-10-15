//! The system's bin: where a deleted file goes on the desktop the app runs
//! on, to be got back from there, as against the vault's own `.trash`.
//!
//! On Windows that is the Recycle Bin, which the shell's own file operation
//! moves things to when it is told the deletion may be undone. No library
//! the app already has makes that call, so it is made here. Elsewhere there
//! is no bin the app knows how to reach yet, and asking for one is an error.

use std::path::Path;

use anyhow::Result;

/// Whether there is a bin to be had here.
pub const AVAILABLE: bool = cfg!(windows);

/// Move the file or folder at `path`, which must exist, to the system's bin.
#[cfg(windows)]
pub fn recycle(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    /// The shell's description of a file operation (`SHFILEOPSTRUCTW`), as
    /// it is laid out in a 64-bit program. In a 32-bit one it is packed.
    #[repr(C)]
    struct FileOperation {
        window: isize,
        operation: u32,
        from: *const u16,
        to: *const u16,
        flags: u16,
        aborted: i32,
        mappings: *mut std::ffi::c_void,
        progress_title: *const u16,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(operation: *mut FileOperation) -> i32;
    }
    const DELETE: u32 = 3;
    // Into the bin, without asking, showing or reporting anything: the app
    // has asked already and reports for itself.
    const SILENT: u16 = 0x0004;
    const NO_CONFIRMATION: u16 = 0x0010;
    const ALLOW_UNDO: u16 = 0x0040;
    const NO_ERROR_UI: u16 = 0x0400;
    anyhow::ensure!(cfg!(target_pointer_width = "64"), "the Recycle Bin is only reached from a 64-bit build");
    anyhow::ensure!(path.exists(), "'{}' is not there", path.display());
    // The shell takes a list of paths, each ended by a nought and the list by
    // another, and wants them whole.
    let whole = std::path::absolute(path)?;
    let mut from: Vec<u16> = whole.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut operation = FileOperation {
        window: 0,
        operation: DELETE,
        from: from.as_ptr(),
        to: std::ptr::null(),
        flags: SILENT | NO_CONFIRMATION | ALLOW_UNDO | NO_ERROR_UI,
        aborted: 0,
        mappings: std::ptr::null_mut(),
        progress_title: std::ptr::null(),
    };
    // Safety: the structure is laid out as the shell's is in a 64-bit
    // program (checked above), the list of paths it points to is ended as
    // the shell wants and outlives the call, and nothing else is pointed to.
    let failed = unsafe { SHFileOperationW(&mut operation) };
    anyhow::ensure!(failed == 0 && operation.aborted == 0, "the Recycle Bin did not take '{}' (error {failed:#x})", path.display());
    anyhow::ensure!(!path.exists(), "'{}' is still there after moving it to the Recycle Bin", path.display());
    Ok(())
}

#[cfg(not(windows))]
pub fn recycle(path: &Path) -> Result<()> {
    anyhow::bail!("there is no system bin the app can reach here, so '{}' was left where it is", path.display())
}
