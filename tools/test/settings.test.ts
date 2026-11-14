// Settings in categories (#168): which tab a card goes under, and the tab
// this device remembers.
import { afterEach, describe, expect, it } from "vitest";
import {
  SETTINGS_TABS, SETTINGS_TAB_KEY, UPSTREAM_CARD_TABS, availableTabs, headingOf, parseTab, readTab, tabForHeading,
  writeTab,
} from "../../src/components/settings/categories";

// A React element, as headingOf sees one.
const el = (type: unknown, children?: unknown) => ({ type, props: children === undefined ? {} : { children } });
const icon = { render: () => null };

describe("which tab a card goes under", () => {
  it("puts the original project's cards where they fit", () => {
    expect(tabForHeading("UI Customization")).toBe("general");
    expect(tabForHeading("Maintenance")).toBe("general");
    expect(tabForHeading("Terminal Configuration")).toBe("terminal");
    expect(tabForHeading("Authentication")).toBe("terminal");
    expect(tabForHeading("Cloud Sync")).toBe("sync");
  });

  it("finds a card whose heading shows in another language", () => {
    const de: Record<string, string> = { "Terminal Configuration": "Terminal-Konfiguration", "Cloud Sync": "Cloud-Synchronisierung" };
    const translate = (s: string) => de[s] ?? s;
    expect(tabForHeading("Terminal-Konfiguration", translate)).toBe("terminal");
    expect(tabForHeading("Cloud-Synchronisierung", translate)).toBe("sync");
    expect(tabForHeading("UI Customization", translate)).toBe("general");
    expect(tabForHeading("Etwas Neues", translate)).toBe("general");
  });

  it("shows a card it doesn't know under General, so none is lost", () => {
    expect(tabForHeading("Something New")).toBe("general");
    expect(tabForHeading(null)).toBe("general");
    expect(tabForHeading("constructor")).toBe("general");
    expect(tabForHeading("toString")).toBe("general");
  });

  it("maps every card heading SettingsPanel.tsx has, and only those", () => {
    const src = import.meta.glob<string>("/src/components/SettingsPanel.tsx", { query: "?raw", import: "default", eager: true })[
      "/src/components/SettingsPanel.tsx"
    ];
    // Each heading is the English text in t("…") (src/i18n), or plain text.
    const headings = [...src.matchAll(/tracking-widest text-xs">\s*<\w+ size=\{14\} \/>\s*(?:\{t\("([^"]+)"\)\}|([^<{]+?))\s*<\/div>/g)].map((m) => m[1] ?? m[2]);
    expect(headings.length).toBeGreaterThan(0);
    expect(headings.filter((h) => !(h in UPSTREAM_CARD_TABS)), "cards with no tab: add them to UPSTREAM_CARD_TABS").toEqual([]);
    expect(Object.keys(UPSTREAM_CARD_TABS).filter((h) => !headings.includes(h)), "headings no card has").toEqual([]);
  });
});

describe("a card's heading", () => {
  it("is the text of a section's first element", () => {
    const card = el("section", [el("div", [el(icon, undefined), " UI Customization"]), el("div", "body text")]);
    expect(headingOf(card)).toBe("UI Customization");
  });

  it("reads text inside nested elements and numbers", () => {
    expect(headingOf(el("section", el("div", [el("span", "Cloud"), " ", el("b", ["Sync ", 2])])))).toBe("Cloud Sync 2");
  });

  it("is null for anything that isn't a section with a heading", () => {
    expect(headingOf(el("div", [el("div", "Not a card")]))).toBeNull();
    expect(headingOf(el(icon, undefined))).toBeNull();
    expect(headingOf(el("section", [el("div", [el(icon, undefined)])]))).toBeNull();
    expect(headingOf(el("section", "just text"))).toBeNull();
    expect(headingOf(null)).toBeNull();
    expect(headingOf(false)).toBeNull();
    expect(headingOf("section")).toBeNull();
  });
});

describe("the tabs", () => {
  it("are six, each once", () => {
    const ids = SETTINGS_TABS.map((t) => t.id);
    expect(ids).toEqual(["general", "terminal", "servers", "ai", "sync", "help"]);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("leave out AI tools on a phone or tablet", () => {
    expect(availableTabs(false)).toContain("ai");
    expect(availableTabs(true)).not.toContain("ai");
    expect(availableTabs(true)).toHaveLength(SETTINGS_TABS.length - 1);
  });
});

describe("the remembered tab", () => {
  const all = availableTabs(false);
  afterEach(() => {
    delete (globalThis as any).localStorage;
  });

  it("is one of the tabs shown, or General", () => {
    expect(parseTab("sync", all)).toBe("sync");
    expect(parseTab("help", all)).toBe("help");
    expect(parseTab("ai", availableTabs(true))).toBe("general");
    expect(parseTab("Sync", all)).toBe("general");
    expect(parseTab("", all)).toBe("general");
    expect(parseTab(null, all)).toBe("general");
    expect(parseTab(3, all)).toBe("general");
    expect(parseTab("__proto__", all)).toBe("general");
  });

  it("is kept on this device", () => {
    const store = new Map<string, string>();
    (globalThis as any).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
    expect(readTab(all)).toBe("general");
    writeTab("terminal");
    expect(store.get(SETTINGS_TAB_KEY)).toBe("terminal");
    expect(readTab(all)).toBe("terminal");
    store.set(SETTINGS_TAB_KEY, "nonsense");
    expect(readTab(all)).toBe("general");
  });

  it("falls back to General when storage refuses", () => {
    (globalThis as any).localStorage = {
      getItem: () => { throw new Error("denied"); },
      setItem: () => { throw new Error("denied"); },
    };
    expect(() => writeTab("help")).not.toThrow();
    expect(readTab(all)).toBe("general");
  });

  it("falls back to General with no storage at all", () => {
    expect(readTab(all)).toBe("general");
    expect(() => writeTab("help")).not.toThrow();
  });
});
