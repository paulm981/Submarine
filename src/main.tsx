import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import ErrorBoundary from "./components/ErrorBoundary";
import "./App.css";
// Registers the bundled terminal fonts (@font-face) before any terminal mounts.
import "./util/bundledFonts";
import { startDiagnostics } from "./util/diagnostics";
import { tameNativeMenus } from "./util/nativeMenus";
import { keepTypingPlain } from "./util/plainTyping";

// The app's log and freeze watchdog (util/diagnostics.ts): a failure that
// escapes React (an async reject, a listener throw), console.error, and a
// window that stops answering all end up in the log file. The ErrorBoundary
// below covers render-time throws.
startDiagnostics();
// No webview menu with "Reload" on a stray right-click (util/nativeMenus.ts).
tameNativeMenus();
// No capitals or corrections the keyboard adds in the app's fields
// (util/plainTyping.ts).
keepTypingPlain();

// ── Android soft-keyboard sizing (visualViewport → --vv-h) ──────────────────
// The Android on-screen keyboard covers our fixed-bottom UI (MobileKeyBar,
// Sidebar dock) unless the layout root shrinks with the keyboard. The
// AndroidManifest sets `windowSoftInputMode="adjustResize"` which shrinks
// the Activity window on every OS version we support — but on older
// WebViews (pre-Chromium M139, mid-2025) `100vh` occasionally lags a frame
// behind the resize. Belt-and-braces: mirror `window.visualViewport.height`
// into a `--vv-h` CSS variable that `#root` reads (see index.html). On
// desktop the value equals `innerHeight` and never changes, so there's zero
// runtime cost.
//
// Fires on:
//   - visualViewport.resize   → keyboard open/close, pinch-zoom
//   - visualViewport.scroll   → URL bar collapse / IME partial slide
//   - window.resize           → window resize (desktop)
//   - window.orientationchange → device rotation (kb usually re-computes)
function syncVvHeight() {
  const vv = window.visualViewport;
  const h = vv ? vv.height : window.innerHeight;
  document.documentElement.style.setProperty("--vv-h", `${h}px`);
}
syncVvHeight();
if (window.visualViewport) {
  window.visualViewport.addEventListener("resize", syncVvHeight);
  window.visualViewport.addEventListener("scroll", syncVvHeight);
}
window.addEventListener("resize", syncVvHeight);
window.addEventListener("orientationchange", syncVvHeight);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <ErrorBoundary>
    <App />
  </ErrorBoundary>,
);
