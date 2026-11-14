import { invoke } from "@tauri-apps/api/core";
import { type ClientImport, type ImportedHost, type ImportedSshKey, type ImportReport, type ImportRow, errText, plural } from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's import of sessions from MobaXterm, PuTTY, ~/.ssh/config or JSON
// (#163): the parts of AddNodePanel's import that the fork does its own way,
// each called from AddNodePanel in one line where its code was.
// ─────────────────────────────────────────────────────────────────────────────

type SetText = (text: string | null) => void;

/** `parse_client_import` on a pasted (or chosen) export: its hosts, and the
 *  line about the sessions it can't import (MobaXterm VNC…). In
 *  parsePastedImport, in place of its `invoke` and `setImportHosts`. */
export const parseClientImport = async (text: string, setHosts: (hosts: ImportedHost[]) => void, setSkipped: SetText) => {
  const res = await invoke<ClientImport>("parse_client_import", { text });
  setHosts(res.hosts);
  setSkipped(res.skipped);
};

/** Read an export file (MobaXterm .mxtsessions, PuTTY .reg, JSON) through
 *  the native dialog and parse it straight away, as if it had been pasted. */
export const chooseImportFile = async (setError: SetText, setText: (text: string) => void, parse: (text: string) => Promise<void>) => {
  setError(null);
  try {
    const text = await invoke<string | null>("pick_client_import_file");
    if (text == null) return;
    setText(text);
    await parse(text);
  } catch (e) {
    setError(errText(e));
  }
};

/** The rest of importSelectedHosts, once the picked hosts and whether their
 *  key files go too (`withKeys`) are known, and whether the passwords
 *  FileZilla kept go too (`withPasswords`, asked in a shared profile).
 *
 *  Load each distinct key file into the vault (import_ssh_key_file dedupes
 *  by content), then hand every picked host to `import_hosts`, which writes
 *  folders, Logins, jump hosts and servers in one transaction. A host whose
 *  key can't be read still imports, signing in without it; the summary says
 *  which key and why (a PuTTY .ppk, say, needs exporting as OpenSSH first). */
export const importPicked = async ({ picked, withKeys, withPasswords = true, keysByPath, setImportBusy, setImportError, setImportDone, refreshServers, refreshFolders, refreshCredentials, refreshSshKeys, onImported }: {
  picked: ImportedHost[];
  withKeys: boolean;
  withPasswords?: boolean;
  keysByPath: Map<string, ImportedSshKey | null>;
  setImportBusy: (busy: boolean) => void;
  setImportError: SetText;
  setImportDone: SetText;
  refreshServers?: () => unknown;
  refreshFolders?: () => unknown;
  refreshCredentials?: () => unknown;
  refreshSshKeys?: () => unknown;
  onImported?: () => void;
}) => {
  const keyFailures = new Map<string, string>();
  const loadKey = async (path: string | null | undefined): Promise<ImportedSshKey | null> => {
    if (!path || !withKeys) return null;
    if (keysByPath.has(path)) return keysByPath.get(path) ?? null;
    let key: ImportedSshKey | null = null;
    try {
      key = await invoke<ImportedSshKey>("import_ssh_key_file", { path, name: null });
    } catch (e) {
      keyFailures.set(path, errText(e));
    }
    keysByPath.set(path, key);
    return key;
  };
  const rows: ImportRow[] = [];
  for (const host of picked) {
    const key = await loadKey(host.identity_file);
    const jumpKey = await loadKey(host.jump_identity_file);
    // A password FileZilla kept goes only when allowed, and not beside a key.
    const dropPassword = !!host.password && (!withPasswords || !!key);
    rows.push({ host: dropPassword ? { ...host, password: undefined } : host, key_id: key ? key.id : null, jump_key_id: jumpKey ? jumpKey.id : null });
  }
  let report: ImportReport | null = null;
  try {
    report = await invoke<ImportReport>("import_hosts", { rows });
  } catch (e) {
    setImportError(errText(e));
  }
  setImportBusy(false);
  if (refreshServers) {
    try { await refreshServers(); } catch { /* refresh failure isn't fatal */ }
  }
  if (refreshFolders) {
    try { await refreshFolders(); } catch { /* same */ }
  }
  if (refreshCredentials) {
    try { await refreshCredentials(); } catch { /* same */ }
  }
  if (keysByPath.size > 0 && refreshSshKeys) {
    try { await refreshSshKeys(); } catch { /* same — the list refreshes on next open */ }
  }
  if (!report) return;
  // Lets the app sync what came in (DesktopApp's bumpSync).
  onImported?.();

  // Anything the user has to finish goes in the summary the modal shows
  // until Done.
  const todo: string[] = [];
  const loaded = [...keysByPath.values()].filter((k): k is ImportedSshKey => !!k);
  const needPassphrase = loaded.filter((k) => k.encrypted).length;
  if (report.logins_created.length > 0) {
    todo.push(
      `${plural(report.logins_created.length, "Login")} created (${report.logins_created.join(", ")}). ` +
      `Fill in each one's user name and password once under Logins, or leave them empty to be asked when connecting.`,
    );
  }
  if (report.jump_hosts_created.length > 0) {
    todo.push(`Jump ${report.jump_hosts_created.length === 1 ? "host" : "hosts"} saved: ${report.jump_hosts_created.join(", ")}.`);
  }
  if (report.multi_hop.length > 0) {
    todo.push(
      `${plural(report.multi_hop.length, "host")} use a chain of jump hosts, which Submarine can't do (one jump host per server): ` +
      `${report.multi_hop.slice(0, 5).join(", ")}${report.multi_hop.length > 5 ? "…" : ""}. They were imported without one.`,
    );
  }
  if (report.rdp_skipped.length > 0) {
    todo.push(`${plural(report.rdp_skipped.length, "Remote Desktop session")} not imported: ${report.rdp_skipped.join("; ")}`);
  }
  for (const [path, why] of keyFailures) {
    const ppk = /\.ppk$/i.test(path) ? " PuTTY .ppk keys need converting first: PuTTYgen › Conversions › Export OpenSSH key (force new file format) — the older format can't be read with a passphrase." : "";
    todo.push(`Couldn't load the key ${path}: ${why}.${ppk} Hosts using it were imported without it.`);
  }
  if (needPassphrase > 0) {
    todo.push(`${plural(needPassphrase, "key")} need a passphrase — add it under Settings › SSH keys, or leave it empty to be asked when connecting.`);
  }
  const summary =
    `${plural(report.added, "server")} imported` +
    (report.folders_created ? `, ${plural(report.folders_created, "folder")} created` : "") +
    (report.jump_linked ? `, ${report.jump_linked} linked to a jump host` : "") +
    (report.rdp_added ? `, ${plural(report.rdp_added, "RDP server")} added` : "") +
    (report.already_there ? `; ${report.already_there} already saved, left as they were` : "") +
    ".";
  setImportDone([summary, ...todo].join("\n\n"));
};
