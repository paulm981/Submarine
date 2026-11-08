import type { ComponentProps } from "react";
import { JumpHostPicker } from "../../components/JumpHostPicker";
import { KeepSessionOverride } from "../../components/KeptSessions";
import { SchemeSwatch } from "../../components/TerminalSchemes";
import { HIGHLIGHT_SETS, highlightSetById, readHighlightId } from "../../ui/highlight";
import { SCHEMES, readSchemeId, schemeById } from "../../ui/terminalThemes";
import { type FolderRow, folderOptions } from "../../util/folderPath";
import { IS_ANDROID } from "../../util/platform";
import { chooseImportFile } from "./hostImport";
import type { ImportedHost, ServerFormNode } from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's pieces of the add/edit server form and its import modal, each
// rendered by AddNodePanel in one line (#163): AddNodePanel.tsx is the
// original project's file. Each renders what AddNodePanel did in its place,
// or nothing where it didn't.
// ─────────────────────────────────────────────────────────────────────────────

type SetNode = (node: ServerFormNode) => void;
type Servers = ComponentProps<typeof JumpHostPicker>["servers"];

/** The folder picker's folders: nested ones by their full path, in path
 *  order (util/folderPath). */
export const FolderOptions = ({ folders }: { folders?: FolderRow[] | null }) => (
  <>{folderOptions(folders || []).map((f) => <option key={f.id} value={f.id.toString()} className="bg-[#1a1a1e] text-white">{f.label}</option>)}</>
);

/** Under the password field, once switching to telnet or VNC took out the
 *  SSH or RDP password (protocolSwitch) and nothing was typed in its place. */
export const HeldPasswordNote = ({ newNode, isTelnet, isVnc }: { newNode: ServerFormNode; isTelnet: boolean; isVnc: boolean }) => (
  <>
    {(isTelnet || isVnc) && newNode.heldPassword?.password && !newNode.password && (
      <p className="text-[10px] text-amber-300/90 ml-1">
        {isVnc
          ? `The ${newNode.heldPassword.from} password was taken out, so it isn't sent over VNC. VNC servers have a password of their own: type it here.`
          : `The ${newNode.heldPassword.from} password was taken out, so it isn't sent over telnet, which doesn't protect it. Type one here if you want it saved.`}
      </p>
    )}
  </>
);

/** Jump via (#102): a search box over the profile's SSH servers, in place of
 *  the original project's drop-down. */
export const JumpVia = ({ isSsh, newNode, setNewNode, servers, folders }: {
  isSsh: boolean;
  newNode: ServerFormNode;
  setNewNode: SetNode;
  servers?: Servers | null;
  folders?: FolderRow[] | null;
}) => (
  <JumpHostPicker
    label={isSsh ? "Jump via" : "Through SSH server"}
    directLabel={isSsh ? "Direct — no jump host" : "Direct — no SSH server"}
    value={newNode.jumpHostId || ""}
    onChange={(id) => setNewNode({ ...newNode, jumpHostId: id })}
    servers={servers || []}
    folders={folders || []}
    selfId={newNode.id}
  />
);

/** The fork's fields at the end of the form: keeping this server's sessions
 *  (when editing an SSH server), and its own terminal colors and
 *  highlighting. */
export const MoreFields = ({ isSsh, isTelnet, isEditMode, newNode, setNewNode, servers }: {
  isSsh: boolean;
  isTelnet: boolean;
  isEditMode: boolean;
  newNode: ServerFormNode;
  setNewNode: SetNode;
  servers?: { id: number; uuid?: string | null }[] | null;
}) => (
  <>
    {isSsh && isEditMode && <KeepSessionOverride uuid={servers?.find((s) => s.id === newNode.id)?.uuid} />}

    {/* A telnet server's tab is a terminal too, so it takes its own
        colors and highlighting like an SSH server's. */}
    {(isSsh || isTelnet) && (<>
    {/* Its own terminal colors (synced with the server), e.g. a light
        scheme for production so it's never mistaken for the rest. */}
    <div className="space-y-1.5 pt-1">
      <label htmlFor="node-terminal-colors" className="text-[11px] font-bold text-zinc-400 ml-1 flex items-center gap-2">
        <span>Terminal colors</span>
        <span className="text-[10px] text-zinc-600 font-normal normal-case tracking-normal">
          Its own, so it stands out. Applies to its open terminals too.
        </span>
      </label>
      <div className="flex items-center gap-2">
        <select
          id="node-terminal-colors"
          value={newNode.terminalTheme || ""}
          onChange={(e) => setNewNode({ ...newNode, terminalTheme: e.target.value })}
          className="flex-1 h-9 bg-[#1a1a1e] rounded-lg px-3 text-[12px] text-white border border-white/10 outline-none focus:border-primary/50 focus:bg-[#232328] transition-all shadow-inner"
        >
          <option value="">Same as Settings ({schemeById(readSchemeId()).name})</option>
          {SCHEMES.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <SchemeSwatch scheme={schemeById(newNode.terminalTheme || readSchemeId())} className="shrink-0 h-9" />
      </div>
    </div>

    {/* Its own highlight set (synced with the server), e.g. Network
        devices for a switch. */}
    <div className="space-y-1.5 pt-1">
      <label htmlFor="node-highlight" className="text-[11px] font-bold text-zinc-400 ml-1 flex items-center gap-2">
        <span>Highlighting</span>
        <span className="text-[10px] text-zinc-600 font-normal normal-case tracking-normal">
          Words and addresses colored in its output. Applies to its open terminals too.
        </span>
      </label>
      <select
        id="node-highlight"
        value={newNode.highlight || ""}
        onChange={(e) => setNewNode({ ...newNode, highlight: e.target.value })}
        className="w-full h-9 bg-[#1a1a1e] rounded-lg px-3 text-[12px] text-white border border-white/10 outline-none focus:border-primary/50 focus:bg-[#232328] transition-all shadow-inner"
      >
        <option value="">Same as Settings ({highlightSetById(readHighlightId()).name})</option>
        {HIGHLIGHT_SETS.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
      </select>
    </div>
    </>)}
  </>
);

/** Choose file… beside Parse pasted text: an export file read through the
 *  native dialog and parsed straight away (chooseImportFile). Not on
 *  Android. */
export const ChooseImportFile = ({ setError, setText, parse }: {
  setError: (text: string | null) => void;
  setText: (text: string) => void;
  parse: (text: string) => Promise<void>;
}) => (
  <>
    {!IS_ANDROID && (
      <button
        onClick={() => chooseImportFile(setError, setText, parse)}
        className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-white/[0.04] text-zinc-300 border border-white/10 hover:bg-white/[0.08] hover:text-white transition-all"
      >
        Choose file…
      </button>
    )}
  </>
);

/** An import row's kind, before its name, when it isn't SSH. */
export const HostBadges = ({ h }: { h: ImportedHost }) => (
  <>
    {h.rdp && <span className="mr-1.5 px-1 py-px rounded text-[9.5px] font-black tracking-wider bg-sky-500/15 text-sky-300 border border-sky-500/30 align-middle">RDP</span>}
    {h.telnet && <span className="mr-1.5 px-1 py-px rounded text-[9.5px] font-black tracking-wider bg-amber-500/15 text-amber-300 border border-amber-500/30 align-middle">TELNET</span>}
  </>
);

/** Under an import row's address: its folder, then its login, key, jump host
 *  and on-connect commands. */
export const HostDetails = ({ h }: { h: ImportedHost }) => (
  <>
    {h.folder && h.folder.length > 0 && (
      <div className="text-[10.5px] text-zinc-500 truncate">{h.folder.join(" › ")}</div>
    )}
    {(h.credential || h.identity_file || h.proxy_jump || h.run_on_connect) && (
      <div className="mt-0.5 text-[10.5px] text-zinc-600 truncate">
        {h.credential && <span className="mr-2">login: {h.credential}</span>}
        {h.identity_file && <span className="mr-2">key: {h.identity_file}</span>}
        {h.proxy_jump && <span className="mr-2">jump: {h.proxy_jump}</span>}
        {h.run_on_connect && <span>runs: {h.run_on_connect}</span>}
      </div>
    )}
  </>
);

/** What the finished import did and what's left to do (useImportResult's
 *  `done`), in place of the host list. */
export const ImportDone = ({ text }: { text: string | null }) => (
  <>
    {text && (
      <div className="m-1 p-3 bg-emerald-500/[0.06] border border-emerald-500/20 rounded-lg text-zinc-200 text-[12px] leading-relaxed whitespace-pre-line">
        {text}
      </div>
    )}
  </>
);

/** The sessions the pasted export holds that can't be imported
 *  (useImportResult's `skipped`), under the host list. */
export const ImportSkipped = ({ text }: { text: string | null }) => (
  <>
    {text && (
      <div className="mt-3 p-2 bg-white/[0.03] border border-white/10 rounded-lg text-zinc-400 text-[11px]">
        {text}
      </div>
    )}
  </>
);

/** The import modal's footer once the import is done. The import was the
 *  point of opening the panel, so Done closes it as well, not just the
 *  import. */
export const ImportDoneButton = ({ onDone }: { onDone: () => void }) => (
  <button
    onClick={onDone}
    className="px-4 h-8 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-primary/15 border border-primary/40 text-primary hover:bg-primary/25"
  >
    Done
  </button>
);
