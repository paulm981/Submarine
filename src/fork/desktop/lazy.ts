import { Suspense, createElement, type ComponentType } from "react";

type ForkUnlocked = typeof import("./unlocked");

let loaded: ForkUnlocked | null = null;
let loading: Promise<ForkUnlocked> | null = null;
let failure: { error: unknown; at: number } | null = null;
const FAILURE_SHOWN_MS = 1000;

/** Starts loading the unlocked fork chunk (and highlight engine) once; resolves with it. */
export function loadForkWorkspace(): Promise<ForkUnlocked> {
  if (!loading) {
    // Also warm the highlight module alongside unlocked components so the first
    // terminal keystroke/output after unlock hits a warm module cache.
    void import("../../ui/highlight").catch(() => {});
    loading = import("./unlocked").then(
      (m) => {
        loaded = m;
        failure = null;
        return m;
      },
      (e) => {
        failure = { error: e, at: Date.now() };
        loading = null;
        throw e;
      },
    );
  }
  return loading;
}

export const forkWorkspaceNow = (): ForkUnlocked | null => loaded;

/**
 * Stand-in component matching `src/workspaceLoader.ts`:
 * - Once `loaded` is ready (prefetched during idle on the profile picker), renders
 *   the real component synchronously with zero Suspense overhead.
 * - While still loading, suspends inside a local `<Suspense fallback={null}>`
 *   so title-bar/overlay callers outside DesktopApp's main Suspense stay safe.
 */
export function fromForkWorkspace<K extends keyof ForkUnlocked>(key: K): ForkUnlocked[K] {
  const Suspender = (props: object) => {
    if (loaded) return createElement(loaded[key] as ComponentType<object>, props);
    if (failure && Date.now() - failure.at < FAILURE_SHOWN_MS) throw failure.error;
    failure = null;
    throw loadForkWorkspace();
  };
  const Part = (props: object) => {
    if (loaded) return createElement(loaded[key] as ComponentType<object>, props);
    return createElement(Suspense, { fallback: null }, createElement(Suspender, props));
  };
  Part.displayName = `ForkWorkspace.${String(key)}`;
  return Part as unknown as ForkUnlocked[K];
}
