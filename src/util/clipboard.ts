import { invoke } from "@tauri-apps/api/core";
import { IS_LINUX, IS_MAC, IS_WINDOWS } from "./platform";

// The clipboard's text, for a paste the app makes itself (a right-click in a
// terminal, the Wall's paste). On Linux and Windows it comes from the backend
// (lib.rs `clipboard_read_text`): WebKitGTK refuses the page's own read, and
// crashes the app on an empty clipboard; WebView2 puts it behind a permission
// prompt that, blocked or dismissed once, fails every paste after it;
// WKWebView on macOS refuses it outright. The keyboard's own paste is the
// webview's and needs neither. Android still reads it in the page.
export const readClipboardText = (): Promise<string> =>
  IS_LINUX || IS_WINDOWS || IS_MAC ? invoke<string>("clipboard_read_text") : navigator.clipboard.readText();
