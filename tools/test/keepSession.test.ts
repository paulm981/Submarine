// Sessions that survive a reconnect (#126): the page's helpers.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  firstKept, keepsSessions, keptSessionDetail, keptSessionLabel, readKeepDefault, readKeepOverride, resolveKeep,
  sortKeptSessions, writeKeepDefault, writeKeepOverride, type KeptSession,
} from "../../src/util/keepSession";
import {
  forgetSessionKeep, readAutoReconnect, readSendKeepalives, writeSessionKeep, AUTO_RECONNECT_KEY, SEND_KEEPALIVES_KEY,
} from "../../src/util/sessionKeep";

const row = (over: Partial<KeptSession>): KeptSession => ({
  name: "submarine-0123abcd-1", pid: null, created: null, attached: false, ours: true, terminal: 1, ...over,
});

describe("whether a server keeps its sessions, on this device", () => {
  let store: Map<string, string>;
  beforeEach(() => {
    store = new Map();
    (globalThis as any).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
  });
  afterEach(() => { delete (globalThis as any).localStorage; });

  it("is the server's own choice, else the default", () => {
    expect(resolveKeep(false, "default")).toBe(false);
    expect(resolveKeep(true, "default")).toBe(true);
    expect(resolveKeep(false, "always")).toBe(true);
    expect(resolveKeep(true, "never")).toBe(false);
  });

  it("is off until turned on, and a server's choice is kept by its sync id", () => {
    expect(readKeepDefault()).toBe(false);
    expect(keepsSessions("u1")).toBe(false);
    writeKeepDefault(true);
    expect(keepsSessions("u1")).toBe(true);
    writeKeepOverride("u1", "never");
    expect(readKeepOverride("u1")).toBe("never");
    expect(keepsSessions("u1")).toBe(false);
    expect(keepsSessions("u2")).toBe(true);
    writeKeepDefault(false);
    writeKeepOverride("u2", "always");
    expect(keepsSessions("u2")).toBe(true);
    writeKeepOverride("u2", "default");
    expect(readKeepOverride("u2")).toBe("default");
    expect(keepsSessions("u2")).toBe(false);
  });

  it("is off for a server with no sync id, or when what's stored doesn't read", () => {
    writeKeepDefault(true);
    expect(keepsSessions(null)).toBe(false);
    expect(keepsSessions(undefined)).toBe(false);
    store.set("submarine-keep-session", "{not json");
    expect(readKeepOverride("u1")).toBe("default");
    store.set("submarine-keep-session", JSON.stringify({ u1: "yes", u2: ["always"] }));
    expect(readKeepOverride("u1")).toBe("default");
    expect(readKeepOverride("u2")).toBe("default");
    store.set("submarine-keep-session", "[\"always\"]");
    expect(readKeepOverride("0")).toBe("default");
  });

  it("says what a kept session means once ever", () => {
    expect(firstKept()).toBe(true);
    expect(firstKept()).toBe(false);
  });

  // The original project's switches for keepalives and reconnecting (their
  // #310, util/sessionKeep) are kept by sync id too, apart from these: turning
  // reconnecting or keepalives off doesn't stop a server keeping its sessions,
  // and forgetting a server's switches doesn't forget its choice here. A
  // kept session then waits on the server until Reconnect is pressed.
  it("is apart from the keepalive and reconnect switches", () => {
    writeKeepDefault(true);
    writeKeepOverride("u1", "always");
    store.set(AUTO_RECONNECT_KEY, "off");
    store.set(SEND_KEEPALIVES_KEY, "off");
    writeSessionKeep("u2", { keepalives: "off", reconnect: "off" });
    expect(readAutoReconnect("u1")).toBe(false);
    expect(readSendKeepalives("u2")).toBe(false);
    expect(keepsSessions("u1")).toBe(true);
    expect(keepsSessions("u2")).toBe(true);

    writeSessionKeep("u1", { keepalives: "on", reconnect: "on" });
    expect(readKeepOverride("u1")).toBe("always");
    forgetSessionKeep("u1");
    expect(readKeepOverride("u1")).toBe("always");
    expect(readAutoReconnect("u1")).toBe(false);

    writeKeepOverride("u2", "never");
    expect(readAutoReconnect("u2")).toBe(false);
    expect(readSendKeepalives("u2")).toBe(false);
  });

  it("is off, and says nothing, where there's no storage at all", () => {
    delete (globalThis as any).localStorage;
    expect(keepsSessions("u1")).toBe(false);
    writeKeepDefault(true);
    expect(readKeepDefault()).toBe(false);
    expect(firstKept()).toBe(false);
  });
});

describe("the sessions list", () => {
  it("names this server's sessions by slot, and another server's by name", () => {
    expect(keptSessionLabel(row({ terminal: 12 }))).toBe("Session 12");
    expect(keptSessionLabel(row({ ours: false, name: "submarine-ffffffff-2" }))).toBe("submarine-ffffffff-2 (another saved server)");
  });

  it("says whether it's open, and when it started where that's known", () => {
    expect(keptSessionDetail(row({ attached: true }))).toBe("Open now");
    expect(keptSessionDetail(row({ created: 0 }))).toBe("Waiting");
    const at = (d: Date) => d.toISOString();
    expect(keptSessionDetail(row({ created: 1762500000 }), at)).toBe("Waiting · started 2025-11-07T07:20:00.000Z");
  });

  it("puts this server's first, by slot", () => {
    const sorted = sortKeptSessions([
      row({ ours: false, name: "submarine-ffffffff-1", terminal: 1 }),
      row({ terminal: 10, name: "submarine-0123abcd-10" }),
      row({ terminal: 2, name: "submarine-0123abcd-2" }),
      row({ terminal: 1 }),
    ]);
    expect(sorted.map((s) => s.name)).toEqual(["submarine-0123abcd-1", "submarine-0123abcd-2", "submarine-0123abcd-10", "submarine-ffffffff-1"]);
  });
});
