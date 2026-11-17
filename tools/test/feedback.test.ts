// Feedback in the fork: a new issue on the fork's GitHub. Report this reply
// copies the reply and keeps it out of the page's address.
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn(async (_cmd: string, _args?: unknown) => undefined);
vi.mock("@tauri-apps/api/core", () => ({ invoke: (cmd: string, args?: unknown) => invoke(cmd, args) }));

const { FORK_NEW_ISSUE_URL, openOnGithub } = await import("../../src/fork/feedback");

describe("openOnGithub", () => {
  const writeText = vi.fn(async (_text: string) => undefined);
  beforeEach(() => {
    invoke.mockClear();
    writeText.mockReset();
    writeText.mockImplementation(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
  });

  it("copies a reply first, then opens the empty new-issue page", async () => {
    const order: string[] = [];
    writeText.mockImplementation(async () => { order.push("copy"); });
    invoke.mockImplementation(async () => { order.push("open"); return undefined; });
    await openOnGithub({ append: "The reply: ssh root@10.0.0.5" });
    expect(writeText).toHaveBeenCalledWith("The reply: ssh root@10.0.0.5");
    expect(invoke).toHaveBeenCalledWith("open_external_url", { url: FORK_NEW_ISSUE_URL });
    expect(order).toEqual(["copy", "open"]);
  });

  it("never puts the reply in the address, even with a prefill", async () => {
    await openOnGithub({ prefill: "Language: de", append: "secret-host.example" });
    const url = (invoke.mock.calls[0][1] as { url: string }).url;
    expect(url.startsWith(FORK_NEW_ISSUE_URL)).toBe(true);
    expect(url).not.toContain("secret-host");
  });

  it("still opens the page when the copy is refused", async () => {
    writeText.mockImplementation(async () => { throw new Error("denied"); });
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    await openOnGithub({ append: "a reply" });
    expect(invoke).toHaveBeenCalledWith("open_external_url", { url: FORK_NEW_ISSUE_URL });
    err.mockRestore();
  });

  it("copies nothing for plain Feedback", async () => {
    await openOnGithub();
    expect(writeText).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("open_external_url", { url: FORK_NEW_ISSUE_URL });
  });
});
