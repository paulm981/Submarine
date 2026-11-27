// A right click in a terminal does one thing (#280): on a touch screen, the
// long press (their #405: a word's selection, or Paste on a blank cell),
// stopped before xterm's own handler; nothing of the app's in a program that
// has the mouse (htop, mc, vim with mouse=a), which gets the click from xterm;
// and elsewhere, or with Shift held (Option on a Mac), the fork's file-path
// menu on a path or else a paste through the fork's own path (control
// characters left out, "Paste N lines?").
//
// The handler lives inside TerminalView, the original project's component,
// so it is read from the file as it is and run here, with stand-ins for what
// it reaches: a sync that reorders or drops a step fails here.
import { describe, expect, it } from "vitest";
import { transformWithEsbuild } from "vite";

const sources = import.meta.glob<string>("/src/components/TerminalView.tsx", { query: "?raw", import: "default", eager: true });
const source = sources["/src/components/TerminalView.tsx"] ?? "";

/** The text of `const name = …;` (an arrow function, to its closing `};`
 *  at the same indent), as TerminalView has it. */
const declaration = (name: string): string => {
  const start = new RegExp(`^( *)const ${name} = `, "m").exec(source);
  if (!start) throw new Error(`TerminalView has no ${name}`);
  const indent = start[1];
  const from = start.index;
  const line = source.indexOf("\n", from);
  // One line, or a block that closes at its own indent.
  if (source.slice(from, line).trimEnd().endsWith(";")) return source.slice(from, line);
  const end = source.indexOf(`\n${indent}};`, from);
  return source.slice(from, end + indent.length + 3);
};

type World = {
  mac?: boolean;
  mobile?: boolean;
  /** The pointer that pressed last: "mouse" or "touch". */
  pointer?: string;
  /** The program running has turned mouse tracking on. */
  mouseProgram?: boolean;
  shift?: boolean;
  alt?: boolean;
  /** The fork's file-path menu finds a path under the pointer. */
  onPath?: boolean;
  /** Two fingers pinched the text size since the first one landed (their #395). */
  pinched?: boolean;
  /** The long press's timer already made this touch's long press (their #405). */
  pressed?: boolean;
};

const handlerSource = Promise.all(["programHasMouse", "bypassesProgram", "onContextMenu"].map(declaration))
  .then((parts) => transformWithEsbuild(parts.join("\n"), "onContextMenu.ts", { loader: "ts" }))
  .then((out) => `${out.code}\nreturn onContextMenu;`);

/** One right click in `world`: what the app did. */
const rightClick = async (world: World) => {
  const did: string[] = [];
  let defaultPrevented = false;
  const term = { modes: { mouseTrackingMode: world.mouseProgram ? "any" : "none" } };
  const pane = { left: 0, top: 0, width: 800, height: 400 };
  const env = {
    IS_MAC: !!world.mac,
    IS_MOBILE: !!world.mobile,
    lastPointerType: world.pointer ?? "mouse",
    term,
    fork: {},
    terminalRef: { current: { parentElement: { getBoundingClientRect: () => pane } } },
    disabledRef: { current: false },
    pinched: !!world.pinched,
    pressed: !!world.pressed,
    longPress: () => { did.push("long press"); },
    pasteClipboard: async () => { did.push("paste"); },
    Fork: {
      fileMenuAt: (_ev: unknown, t: unknown, _f: unknown, paste: () => void) => {
        expect(t).toBe(term);
        expect(typeof paste).toBe("function");
        if (world.onPath) did.push("file menu");
        return !!world.onPath;
      },
    },
  };
  const make = new Function(...Object.keys(env), await handlerSource) as (...a: unknown[]) => (ev: unknown) => void;
  const onContextMenu = make(...Object.values(env));
  onContextMenu({
    button: 2, clientX: 100, clientY: 100,
    shiftKey: !!world.shift, altKey: !!world.alt, ctrlKey: false, metaKey: false,
    pointerType: world.pointer ?? "mouse",
    preventDefault: () => { defaultPrevented = true; },
    // Stopped: xterm's own handler, after this one, never hears it.
    stopPropagation: () => { did.push("stopped"); },
  });
  // The webview's own menu never shows.
  expect(defaultPrevented).toBe(true);
  return did;
};

describe("a right click in a terminal does one thing (#280, their #371)", () => {
  it("pastes in a plain shell, through the fork's paste", async () => {
    expect(await rightClick({})).toEqual(["paste"]);
  });

  it("opens the file-path menu on a path in a plain shell, and doesn't paste as well", async () => {
    expect(await rightClick({ onPath: true })).toEqual(["file menu"]);
  });

  it("leaves the click to a program that has the mouse: no paste, no menu", async () => {
    expect(await rightClick({ mouseProgram: true })).toEqual([]);
    expect(await rightClick({ mouseProgram: true, onPath: true })).toEqual([]);
    // Option isn't the bypass key off a Mac, nor Shift on one.
    expect(await rightClick({ mouseProgram: true, alt: true })).toEqual([]);
    expect(await rightClick({ mouseProgram: true, mac: true, shift: true })).toEqual([]);
  });

  it("with Shift (Option on a Mac) in such a program, is the terminal's again: the menu or a paste", async () => {
    expect(await rightClick({ mouseProgram: true, shift: true })).toEqual(["paste"]);
    expect(await rightClick({ mouseProgram: true, shift: true, onPath: true })).toEqual(["file menu"]);
    expect(await rightClick({ mouseProgram: true, mac: true, alt: true })).toEqual(["paste"]);
  });

  it("makes only the long press for a touch screen's context menu, and stops it before xterm", async () => {
    expect(await rightClick({ pointer: "touch" })).toEqual(["stopped", "long press"]);
    // Not after a pinch (their #395): the fingers changed the text size.
    expect(await rightClick({ pointer: "touch", pinched: true })).toEqual(["stopped"]);
    // Not twice (their #405): the timer already made this touch's long press.
    expect(await rightClick({ pointer: "touch", pressed: true })).toEqual(["stopped"]);
    expect(await rightClick({ pointer: "touch", onPath: true, mouseProgram: true })).toEqual(["stopped", "long press"]);
    expect(await rightClick({ mobile: true })).toEqual(["stopped", "long press"]);
  });
});
