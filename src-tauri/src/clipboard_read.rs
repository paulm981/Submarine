//! The clipboard's text for a paste into the terminal (a right-click), where
//! the page can't read it itself, on Windows and macOS (lib.rs,
//! clipboard_read_text, reads it on Linux). The desktop frontends all ask;
//! Android reads it in the page.
//!
//! On Windows the page's read is WebView2's to allow, behind a permission
//! prompt: blocked or dismissed once, every right-click paste failed with
//! "Clipboard read denied". The Win32 clipboard is read directly instead
//! (windows_clipboard.rs).
//!
//! On macOS, WKWebView refuses the page's read outright ("Clipboard read
//! denied"), so `pbpaste` reads the general pasteboard.
//!
//! A clipboard that is empty or holds no text gives an empty string.

/// The clipboard's text, on a system other than Linux.
#[cfg(not(target_os = "linux"))]
pub(crate) async fn read_text() -> Result<String, String> {
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(crate::windows_clipboard::windows_clipboard_text)
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(target_os = "macos")]
    {
        let read = tokio::process::Command::new("/usr/bin/pbpaste")
            // It writes the text in the encoding these name, and an app
            // opened from the Finder or the Dock may have none set.
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(std::time::Duration::from_secs(5), read).await {
            Ok(Ok(out)) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
            Ok(Ok(out)) => Err(format!("The clipboard could not be read (pbpaste: {}).", out.status)),
            Ok(Err(e)) => Err(format!("The clipboard could not be read: {}", e)),
            Err(_) => Err("The clipboard did not answer.".to_string()),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("Reading the clipboard here is only for Linux, Windows and macOS.".to_string())
    }
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the line that brings the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod clipboard_read;", 1),
                ("clipboard_read_text", "clipboard_read::read_text().await", 1),
            ],
        );
    }
}
