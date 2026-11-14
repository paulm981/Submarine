import { createPortal } from "react-dom";
import { Activity, ChevronRight, Edit2, Folder, Play } from "lucide-react";
import { OsBadge } from "../../components/OsBadge";
import { pingServers } from "../../ui/ping";
import type { FolderRow } from "../../util/folderPath";
import type { CardMenuAt, MenuServer } from "./hooks";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's pieces of the server grid, each rendered by NodeGrid in one line
// (#163): NodeGrid.tsx is the original project's file. Each renders what
// NodeGrid did in its place, or nothing where it didn't.
// ─────────────────────────────────────────────────────────────────────────────

/** A server's OS badge once the server has reported its OS (server_os.rs),
 *  in place of the protocol icon inside the original project's avatar box:
 *  the box keeps the server's colour and the open tab's dot, and the card's
 *  protocol badge still says what kind of server it is. */
export const OsAvatar = ({ os, size }: { os: string; size: number }) => <OsBadge os={os} size={size} />;

/** The header's path to the open folder: All, then each folder down to it;
 *  each step above it opens that level. */
export const FolderPath = ({ path, setActiveFolderId }: { path: FolderRow[]; setActiveFolderId: (id: number | null) => void }) => (
  <h2 className="text-[15px] font-bold text-primary tracking-tight flex items-center gap-1.5 flex-1 min-w-0 flex-wrap">
    <Folder size={16} className="shrink-0" />
    <button onClick={() => setActiveFolderId(null)} className="text-zinc-500 hover:text-zinc-200 font-semibold">All</button>
    {path.map((c, i) => (
      <span key={c.id} className="flex items-center gap-1.5 min-w-0">
        <ChevronRight size={12} className="text-zinc-600 shrink-0" />
        {i === path.length - 1
          ? <span className="truncate">{c.name}</span>
          : <button onClick={() => setActiveFolderId(c.id)} className="text-zinc-400 hover:text-zinc-100 font-semibold truncate">{c.name}</button>}
      </span>
    ))}
  </h2>
);

/** A server card's menu (useCardMenu): Open, Ping, Edit. The layer under it
 *  takes the click that closes it, so that click doesn't also open the card
 *  beneath. */
export const CardMenu = ({ menu, close, onOpenServer, onEditServer }: {
  menu: CardMenuAt | null;
  close: () => void;
  onOpenServer: (s: MenuServer) => void;
  onEditServer: (s: MenuServer) => void;
}) => (
  <>
    {menu && createPortal(
      <>
        <div
          className="fixed inset-0 z-[9998]"
          onClick={() => close()}
          onContextMenu={(e) => { e.preventDefault(); close(); }}
        />
        <div
          className="fixed z-[9999] w-[200px] py-1 bg-[#15151a] border border-white/10 rounded-lg shadow-2xl text-[11.5px]"
          style={{ left: Math.min(menu.x, window.innerWidth - 220), top: Math.min(menu.y, window.innerHeight - 120) }}
        >
          {([
            { icon: Play, label: "Open", act: () => onOpenServer(menu.server) },
            { icon: Activity, label: "Ping", act: () => pingServers([menu.server]) },
            { icon: Edit2, label: "Edit…", act: () => onEditServer(menu.server) },
          ] as const).map(({ icon: Icon, label, act }) => (
            <button
              key={label}
              onClick={() => { close(); act(); }}
              className="w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left"
            >
              <Icon size={13} className="text-zinc-500" /> {label}
            </button>
          ))}
        </div>
      </>,
      document.body
    )}
  </>
);
