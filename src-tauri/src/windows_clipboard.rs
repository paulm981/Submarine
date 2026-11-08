//! The Windows clipboard's text, read through Win32.
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. clipboard_read.rs calls it.

/// The Win32 clipboard's text (CF_UNICODETEXT).
#[cfg(windows)]
pub(crate) fn windows_clipboard_text() -> Result<String, String> {
    use std::ffi::c_void;
    const CF_UNICODETEXT: u32 = 13;
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(owner: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(mem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(mem: *mut c_void) -> i32;
        fn GlobalSize(mem: *mut c_void) -> usize;
    }
    struct Opened;
    impl Drop for Opened {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }
    // Only one program can have the clipboard open, and some hold it for a
    // moment (clipboard managers, Remote Desktop sharing the clipboard).
    let mut opened = None;
    for _ in 0..10 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = Some(Opened);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let Some(_opened) = opened else {
        return Err("Another program is using the clipboard — try again.".to_string());
    };
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
        return Ok(String::new());
    }
    let mem = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if mem.is_null() {
        return Ok(String::new());
    }
    let ptr = unsafe { GlobalLock(mem) } as *const u16;
    if ptr.is_null() {
        return Ok(String::new());
    }
    // Up to the first NUL, and never past the block's own size.
    let units = unsafe { GlobalSize(mem) } / 2;
    let wide = unsafe { std::slice::from_raw_parts(ptr, units) };
    let len = wide.iter().position(|&c| c == 0).unwrap_or(units);
    let text = String::from_utf16_lossy(&wide[..len]);
    unsafe { GlobalUnlock(mem) };
    Ok(text)
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks("lib.rs", include_str!("lib.rs"), &[("", "mod windows_clipboard;", 1)]);
        assert_hooks(
            "clipboard_read.rs",
            include_str!("clipboard_read.rs"),
            &[("read_text", "windows_clipboard::windows_clipboard_text", 1)],
        );
    }
}
