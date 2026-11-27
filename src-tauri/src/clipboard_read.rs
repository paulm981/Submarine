//! The clipboard's text for a paste into the terminal (a right-click), where
//! the page can't read it itself, on macOS (lib.rs, clipboard_read_text,
//! reads it on Windows, Linux and Android). The desktop frontends all ask.
//!
//! On macOS, WKWebView refuses the page's read outright ("Clipboard read
//! denied"), so `pbpaste` reads the general pasteboard.
//!
//! Windows was read here too until the original project read it in the
//! backend itself (their #418, file_clipboard.rs): the fork uses theirs.
//!
//! A clipboard that is empty or holds no text gives an empty string.

/// The clipboard's text, on a system other than Windows, Linux and Android.
#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
pub(crate) async fn read_text() -> Result<String, String> {
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
    #[cfg(not(target_os = "macos"))]
    {
        Err("Reading the clipboard here is only for Windows, Linux, Android and macOS.".to_string())
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
