import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { Activity, X } from "lucide-react";
import { playBackSound, stopAllPings, stopPing, usePinged, type PingTarget } from "../ui/ping";

// Ping (#33): a small panel, bottom right, with a card for each server being
// pinged. Each card asks the backend for one check (ping_server: a plain TCP
// connect to the server's port, or a channel through its jump host) every
// CHECK_EVERY_MS while its port doesn't answer, and every CHECK_READY_MS once
// it does: each check opens and closes a connection, which a server or jump
// host that bans repeated ones (fail2ban, sshguard) could take for an attack.
// It says when the server stopped answering and when it came back. Closing a
// card stops its checks; nothing keeps running after.

const CHECK_EVERY_MS = 2000;
const CHECK_READY_MS = 30000;
const REPLIES_SHOWN = 8;
const EVENTS_SHOWN = 4;

// What one check found (src-tauri/src/ping.rs, PingReply).
type Reply =
  | { kind: "answers"; port: number; ms: number; through: string | null }
  | { kind: "refused"; port: number }
  | { kind: "no_answer"; port: number; why: string | null; through: string | null }
  | { kind: "not_checked"; why: string };

// ready: its port answers; up: the machine answers, its port doesn't yet;
// down: nothing answers.
type Level = "ready" | "up" | "down";
type Tone = "ok" | "warn" | "bad";
interface Line { key: number; at: number; text: string; tone: Tone }

const PORT_NAMES: Record<string, string> = { rdp: "RDP port", vnc: "VNC port", telnet: "Telnet port" };
const portName = (protocol: string | null) => PORT_NAMES[protocol ?? ""] ?? "SSH port";

const clock = (t: number) => new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

const howLong = (ms: number) => {
  const s = Math.max(1, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return s % 60 ? `${m} min ${s % 60} s` : `${m} min`;
  return m % 60 ? `${Math.floor(m / 60)} h ${m % 60} min` : `${Math.floor(m / 60)} h`;
};

const TONE_TEXT: Record<Tone, string> = { ok: "text-emerald-300", warn: "text-amber-300", bad: "text-rose-300" };
const TONE_DOT: Record<Tone, string> = { ok: "bg-emerald-400", warn: "bg-amber-400", bad: "bg-rose-400" };
const LEVEL_TONE: Record<Level, Tone> = { ready: "ok", up: "warn", down: "bad" };

let lineKey = 0;

const PingCard = ({ target }: { target: PingTarget }) => {
  const port = portName(target.protocol);
  const [replies, setReplies] = useState<Line[]>([]);
  const [events, setEvents] = useState<Line[]>([]);
  const [level, setLevel] = useState<Level | null>(null);
  const [since, setSince] = useState<number>(Date.now());
  const [note, setNote] = useState<string | null>(null);
  const [stopped, setStopped] = useState(false);

  useEffect(() => {
    let alive = true;
    let timer = 0;
    let last: Level | null = null;
    let lastChange = Date.now();
    const event = (text: string, tone: Tone) =>
      setEvents((prev) => [{ key: ++lineKey, at: Date.now(), text, tone }, ...prev].slice(0, EVENTS_SHOWN));
    const reply = (text: string, tone: Tone) =>
      setReplies((prev) => [{ key: ++lineKey, at: Date.now(), text, tone }, ...prev].slice(0, REPLIES_SHOWN));

    const check = async () => {
      const started = Date.now();
      let r: Reply;
      try {
        r = await invoke<Reply>("ping_server", { serverId: target.id });
      } catch (e) {
        // The server is gone (deleted, or the profile closed): nothing to check.
        if (!alive) return;
        setNote(`Stopped: ${e}`);
        setStopped(true);
        return;
      }
      if (!alive) return;
      if (r.kind === "not_checked") {
        // Asked again: a terminal opened to its jump host makes it checkable.
        setNote(r.why);
      } else {
        setNote(null);
        const from = "through" in r && r.through ? ` (from ${r.through})` : "";
        const now: Level = r.kind === "answers" ? "ready" : r.kind === "refused" ? "up" : "down";
        if (r.kind === "answers") reply(`${port} answers in ${r.ms} ms${from}`, "ok");
        else if (r.kind === "refused") reply(`The server answers, its ${port} doesn't yet`, "warn");
        else reply(`No answer${r.why ? ` (${r.why})` : ""}${from}`, "bad");

        if (last !== null && now !== last) {
          const at = clock(Date.now());
          const gone = howLong(Date.now() - lastChange);
          if (now === "down") event(`Stopped answering at ${at}`, "bad");
          else if (last === "down" && now === "up") event(`Back at ${at}, after ${gone}; its ${port} doesn't answer yet`, "warn");
          else if (last === "down" && now === "ready") event(`Back at ${at}, after ${gone}; its ${port} answers`, "ok");
          else if (now === "ready") event(`${port} answers again at ${at}`, "ok");
          else event(`${port} stopped answering at ${at}; the server still answers`, "warn");
          if (now === "ready") playBackSound();
        }
        if (now !== last) {
          lastChange = Date.now();
          setSince(lastChange);
        }
        last = now;
        setLevel(now);
      }
      const every = last === "ready" ? CHECK_READY_MS : CHECK_EVERY_MS;
      timer = window.setTimeout(check, Math.max(0, every - (Date.now() - started)));
    };
    void check();
    return () => { alive = false; window.clearTimeout(timer); };
  }, [target.id, port]);

  const headline =
    level === null ? (note ? null : "Checking…")
    : level === "ready" ? `${port} answers`
    : level === "up" ? `Up since ${clock(since)}, ${port} not answering yet`
    : `Not answering since ${clock(since)}`;

  return (
    <div className="bg-[#15151a] border border-white/10 rounded-xl shadow-2xl overflow-hidden">
      <div className="flex items-center gap-2 px-3 pt-2.5 pb-1.5">
        <span className={`shrink-0 h-2 w-2 rounded-full ${level ? TONE_DOT[LEVEL_TONE[level]] : "bg-zinc-600"} ${level && !stopped ? "animate-pulse" : ""}`} />
        <div className="min-w-0 flex-1">
          <div className="text-[12.5px] font-semibold text-zinc-100 truncate" title={target.name}>{target.name}</div>
          <div className="text-[10.5px] font-mono text-zinc-500 truncate" title={target.host}>{target.host}</div>
        </div>
        <button
          onClick={() => stopPing(target.id)}
          aria-label={`Stop pinging ${target.name}`}
          title="Stop pinging"
          className="shrink-0 w-6 h-6 flex items-center justify-center rounded text-zinc-500 hover:text-white hover:bg-white/5"
        >
          <X size={13} />
        </button>
      </div>
      <div className="px-3 pb-2.5 space-y-1.5">
        {headline && <div className={`text-[12px] font-semibold ${level ? TONE_TEXT[LEVEL_TONE[level]] : "text-zinc-400"}`}>{headline}</div>}
        {level && !note && (
          <div className="text-[10.5px] text-zinc-500">
            {level === "ready" ? "Checked every 30 seconds while it answers." : "Checked every 2 seconds until it answers."}
          </div>
        )}
        {note && <div className="text-[11.5px] text-zinc-400 leading-snug">{note}</div>}
        {events.length > 0 && (
          <ul className="space-y-0.5">
            {events.map((l) => <li key={l.key} className={`text-[11px] leading-snug ${TONE_TEXT[l.tone]}`}>{l.text}</li>)}
          </ul>
        )}
        {replies.length > 0 && (
          <ul className="pt-1 border-t border-white/5 space-y-0.5">
            {replies.map((l) => (
              <li key={l.key} className="flex gap-2 text-[10.5px] leading-snug">
                <span className="shrink-0 font-mono text-zinc-600">{clock(l.at)}</span>
                <span className={l.tone === "ok" ? "text-zinc-300" : TONE_TEXT[l.tone]}>{l.text}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
};

export const PingPanel = () => {
  const pinged = usePinged();
  // Pings end with the profile (this panel goes when it locks).
  useEffect(() => () => stopAllPings(), []);
  if (pinged.length === 0) return null;
  return createPortal(
    <div className="fixed bottom-4 right-4 z-[80] w-[300px] max-w-[calc(100vw-2rem)] max-h-[70vh] flex flex-col gap-2" aria-label="Ping">
      <div className="flex items-center gap-2 px-1 text-[10.5px] font-bold uppercase tracking-widest text-zinc-500">
        <Activity size={12} className="text-primary" /> Ping
        {pinged.length > 1 && (
          <button onClick={stopAllPings} className="ml-auto normal-case tracking-normal font-semibold text-zinc-400 hover:text-white">
            Stop all
          </button>
        )}
      </div>
      <div className="flex flex-col gap-2 overflow-y-auto custom-scrollbar">
        {pinged.map((t) => <PingCard key={t.id} target={t} />)}
      </div>
    </div>,
    document.body,
  );
};

export default PingPanel;
