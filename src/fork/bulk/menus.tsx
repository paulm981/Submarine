import { ArrowDownToLine, ArrowUpFromLine, Loader2 } from "lucide-react";
import { IS_MOBILE } from "../../util/platform";
import { canBulk, openBulk, useBulkRunning } from "./store";

// ─────────────────────────────────────────────────────────────────────────────
// Bulk push and pull (#159) in the right-click menus: a tab's, and the
// session tree's for servers with a session open. A run that's going is shown
// again from either. On a phone or tablet there's no folder to pick.
// ─────────────────────────────────────────────────────────────────────────────

/** The items, for the sessions `from`: none when none can copy. */
export const BulkItems = ({ from, className, iconClass = "text-zinc-500", onPick }: {
  from: string[]; className: string; iconClass?: string; onPick: () => void;
}) => {
  const running = useBulkRunning();
  if (IS_MOBILE) return null;
  if (running) {
    return (
      <button className={className} onClick={() => { onPick(); openBulk("push", from); }}>
        <Loader2 size={12} className={`${iconClass} animate-spin`} /> Show the file copy
      </button>
    );
  }
  if (from.length === 0) return null;
  return (
    <>
      <button className={className} onClick={() => { onPick(); openBulk("push", from); }}>
        <ArrowUpFromLine size={12} className={iconClass} /> Push files to servers…
      </button>
      <button className={className} onClick={() => { onPick(); openBulk("pull", from); }}>
        <ArrowDownToLine size={12} className={iconClass} /> Pull files from servers…
      </button>
    </>
  );
};

/** In a tab's right-click menu. */
export const BulkTabItems = ({ targetId, sessions, statuses, onDone }: {
  targetId: string; sessions: { id: string; protocol?: string }[]; statuses: Record<string, string>; onDone: () => void;
}) => {
  const target = sessions.find((s) => s.id === targetId);
  return (
    <BulkItems
      from={canBulk(target, statuses[targetId]) ? [targetId] : []}
      className="w-full text-left px-3 py-1.5 hover:bg-primary/15 hover:text-primary text-zinc-200 flex items-center gap-2"
      onPick={onDone}
    />
  );
};
