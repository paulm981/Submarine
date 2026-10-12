// The dot beside a session in the session tree — the same one the tab strip
// draws (DesktopApp, kept inline there as upstream wrote it): its
// connection state, outranked by a prompt waiting on the user (host key, 2FA,
// login), which pulses in the accent colour until it's answered. The pulse
// otherwise only runs while connecting, so a steady-state tab doesn't draw
// the eye every half second.
export const sessionDot = (st: string, waiting: boolean): { tone: string; title: string } => ({
  tone:
    waiting               ? "bg-primary animate-pulse" :
    st === "connected"    ? "bg-emerald-400" :
    st === "connecting"   ? "bg-amber-400 animate-pulse" :
    st === "failed"       ? "bg-rose-500" :
    st === "disconnected" ? "bg-rose-500" :
                            "bg-zinc-500",
  title:
    waiting               ? "Waiting for you — open this tab to answer" :
    st === "connected"    ? "Connected" :
    st === "connecting"   ? "Connecting…" :
    st === "failed"       ? "Connection failed" :
    st === "disconnected" ? "Disconnected" :
                            st,
});
