use std::path::PathBuf;

/// Live working directory of process `pid`, read in a single
/// `proc_pidinfo(PROC_PIDVNODEPATHINFO)` syscall (macOS). Returns `None` on any
/// failure — a dead or recycled pid, denied access, or a short read — so callers
/// fall back to the pane's spawn cwd (terminal.md §2, §12).
pub fn live_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            std::ptr::addr_of_mut!(info).cast::<libc::c_void>(),
            size,
        )
    };
    if written < size {
        return None;
    }
    let raw = &info.pvi_cdir.vip_path;
    let bytes = unsafe {
        std::slice::from_raw_parts(raw.as_ptr().cast::<u8>(), std::mem::size_of_val(raw))
    };
    let len = bytes.iter().position(|&b| b == 0)?;
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(OsStr::from_bytes(&bytes[..len])))
}
