// The fork's parts of the add/edit server form (src/fork/server-form, moved out
// of AddNodePanel.tsx, the original project's file, for #163), checked against
// the code as it was in AddNodePanel before the move, kept here as `before…`:
// the form's fields after a switch of kind, the import's calls to the backend
// (`import_hosts`' rows above all) and what it reports, byte for byte.
import { beforeEach, describe, expect, it, vi } from "vitest";

// Each call as sent: its arguments as JSON text, so the order of their keys
// counts too.
const calls: { cmd: string; args: string }[] = [];
let answer: (cmd: string, args: unknown) => unknown = () => undefined;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args?: unknown) => {
    calls.push({ cmd, args: JSON.stringify(args ?? null) });
    const a = answer(cmd, args);
    if (a instanceof Error) throw a.message;
    return a;
  },
}));

import { invoke } from "@tauri-apps/api/core";
import { chooseImportFile, importPicked, parseClientImport } from "../../src/fork/server-form/hostImport";
import { type ImportedHost, type ImportedSshKey, type ImportReport, type ImportRow, folderParentId, plural, protocolSwitch, rowKey } from "../../src/fork/server-form/rules";
import { deleteFolderMessage } from "../../src/fork/grid/rules";
import { jumpHostNote } from "../../src/util/jumpHostNote";

type Node = Record<string, unknown> & {
  protocol?: string; port?: number | ""; authType?: string; credentialId?: unknown; password?: string; password_dirty?: boolean;
  heldPassword?: { from: string; password: string; dirty: boolean };
};
type Protocol = "ssh" | "rdp" | "telnet" | "vnc";

// AddNodePanel's setProtocol before the move, giving what it set.
const beforeSetProtocol = (newNode: Node, protocol: Protocol): Node | undefined => {
  if (protocol === newNode.protocol) return;
  const usual: Record<string, number> = { ssh: 22, rdp: 3389, telnet: 23, vnc: 5900 };
  const port = !newNode.port || Object.values(usual).includes(newNode.port) ? usual[protocol] : newNode.port;
  const dropsLogin = (protocol !== "ssh" && newNode.authType === "custom_key") || (protocol === "vnc" && newNode.authType === "vault");
  const authType = dropsLogin ? "custom_pass" : newNode.authType;
  const credentialId = protocol === "vnc" ? "" : newNode.credentialId;
  const weak = (p: string | undefined) => p === "vnc" || p === "telnet";
  let password = {};
  if (!weak(newNode.protocol) && weak(protocol)) {
    password = {
      password: "",
      password_dirty: true,
      heldPassword: {
        from: newNode.protocol === "rdp" ? "RDP" : "SSH",
        password: newNode.password || "",
        dirty: !!newNode.password_dirty,
      },
    };
  } else if (weak(newNode.protocol) && !weak(protocol) && newNode.heldPassword) {
    password = {
      password: newNode.heldPassword.password,
      password_dirty: newNode.heldPassword.dirty,
      heldPassword: undefined,
    };
  }
  return { ...newNode, protocol, port, authType, credentialId, ...password };
};

// AddNodePanel's setProtocol after it.
const afterSetProtocol = (newNode: Node, protocol: Protocol): Node | undefined => {
  if (protocol === newNode.protocol) return;
  const usual: Record<string, number> = { ssh: 22, rdp: 3389, telnet: 23, vnc: 5900 };
  const port = !newNode.port || Object.values(usual).includes(newNode.port) ? usual[protocol] : newNode.port;
  return { ...newNode, protocol, port, ...protocolSwitch(newNode, protocol) };
};

describe("switching a server's kind", () => {
  const protocols: (Protocol | undefined)[] = [undefined, "ssh", "rdp", "telnet", "vnc"];
  const nodes: Node[] = [];
  for (const protocol of protocols)
    for (const authType of ["vault", "custom_pass", "custom_key", undefined])
      for (const password of ["", "hunter2", undefined])
        for (const held of [undefined, { from: "RDP", password: "kept", dirty: true }, { from: "SSH", password: "", dirty: false }])
          for (const port of [22, 2222, "" as const, undefined])
            nodes.push({
              name: "web", host: "10.0.0.1", ...(protocol ? { protocol } : {}), port, authType, credentialId: "7", folderId: "3",
              password, password_dirty: password === "hunter2", ...(held ? { heldPassword: held } : {}), terminalTheme: "",
            });
  // And forms without the fields a switch sets, so the order it adds them in counts.
  for (const protocol of protocols)
    for (const authType of ["vault", "custom_key", undefined])
      nodes.push({ ...(protocol ? { protocol } : {}), ...(authType ? { authType } : {}), name: "bare" });

  it("sets the same fields, with the same values, in the same order", () => {
    let compared = 0;
    for (const node of nodes) {
      for (const to of ["ssh", "rdp", "telnet", "vnc"] as Protocol[]) {
        const before = beforeSetProtocol(node, to);
        const after = afterSetProtocol(node, to);
        // Entries, not toEqual: the order of the keys, and keys set to
        // undefined, count too.
        expect(after && Object.entries(after)).toEqual(before && Object.entries(before));
        compared++;
      }
    }
    expect(compared).toBe(nodes.length * 4);
  });

  it("holds an SSH password over telnet and brings it back", () => {
    const telnet = afterSetProtocol({ protocol: "ssh", port: 22, authType: "custom_pass", password: "pw", password_dirty: false }, "telnet")!;
    expect(telnet).toMatchObject({ password: "", password_dirty: true, heldPassword: { from: "SSH", password: "pw", dirty: false }, port: 23 });
    expect(afterSetProtocol(telnet, "ssh")).toMatchObject({ password: "pw", password_dirty: false, heldPassword: undefined, port: 22 });
  });

  // Plain FTP sends the password as plain text, as telnet does (#246).
  it("holds an SSH or FTPS password over plain FTP, and keeps it for FTPS", () => {
    const ssh = { protocol: "ssh", port: 22, authType: "custom_pass", password: "pw", password_dirty: false };
    const plain = { ...ssh, ...protocolSwitch(ssh, "ftp"), protocol: "ftp" };
    expect(plain).toMatchObject({ password: "", password_dirty: true, heldPassword: { from: "SSH", password: "pw", dirty: false } });
    expect({ ...plain, ...protocolSwitch(plain, "ftps") }).toMatchObject({ password: "pw", password_dirty: false, heldPassword: undefined });
    // FTPS checks the server's certificate first: the password stays.
    expect(protocolSwitch(ssh, "ftps")).toEqual({ authType: "custom_pass", credentialId: undefined });
    expect(protocolSwitch(ssh, "ftps-implicit")).toEqual({ authType: "custom_pass", credentialId: undefined });
    const ftps = { protocol: "ftps-implicit", authType: "custom_pass", password: "tls", password_dirty: true };
    expect(protocolSwitch(ftps, "ftp")).toMatchObject({ password: "", heldPassword: { from: "FTPS", password: "tls", dirty: true } });
    // A saved login stays (ftp_connect signs in with it, as telnet does);
    // an SSH key doesn't.
    expect(protocolSwitch({ protocol: "ssh", authType: "vault", credentialId: "7" }, "ftp")).toMatchObject({ authType: "vault", credentialId: "7" });
    expect(protocolSwitch({ protocol: "ssh", authType: "custom_key" }, "ftps")).toMatchObject({ authType: "custom_pass" });
  });
});

describe("a new folder from the form", () => {
  it("goes inside the folder picked, or at the top level", () => {
    expect(folderParentId({ folderId: "12" })).toBe(12);
    expect(folderParentId({ folderId: "" })).toBeNull();
    expect(folderParentId({})).toBeNull();
  });
});

// AddNodePanel's importSelectedHosts before the move, from its picked hosts on.
const beforeImport = async ({ importHosts, importSelected, importBusy, shared, answerConfirm, setImportBusy, setImportError, setImportDone, refreshServers, refreshFolders, refreshCredentials, refreshSshKeys, onImported }: {
  importHosts: ImportedHost[]; importSelected: Set<string>; importBusy: boolean; shared: boolean; answerConfirm: boolean;
  setImportBusy: (b: boolean) => void; setImportError: (e: string | null) => void; setImportDone: (d: string | null) => void;
  refreshServers?: () => unknown; refreshFolders?: () => unknown; refreshCredentials?: () => unknown; refreshSshKeys?: () => unknown; onImported?: () => void;
}) => {
  if (importSelected.size === 0 || importBusy) return;
  setImportBusy(true);
  setImportError(null);
  const picked = importHosts.filter((h) => importSelected.has(rowKey(h)));
  const keysByPath = new Map<string, ImportedSshKey | null>();
  const keyFailures = new Map<string, string>();
  const keyFiles = [
    ...new Set(picked.flatMap((h) => [h.identity_file, h.jump_identity_file]).filter((p): p is string => !!p)),
  ];
  let withKeys = true;
  if (keyFiles.length > 0) {
    const isShared = await invoke<{ current: string | null }>("share_key_state").then((s) => !!s.current).catch(() => false);
    if (isShared && shared) withKeys = answerConfirm;
  }
  const loadKey = async (path: string | null | undefined): Promise<ImportedSshKey | null> => {
    if (!path || !withKeys) return null;
    if (keysByPath.has(path)) return keysByPath.get(path) ?? null;
    let key: ImportedSshKey | null = null;
    try {
      key = await invoke<ImportedSshKey>("import_ssh_key_file", { path, name: null });
    } catch (e) {
      keyFailures.set(path, typeof e === "string" ? e : ((e as { message?: string } | null)?.message || String(e)));
    }
    keysByPath.set(path, key);
    return key;
  };
  const rows: ImportRow[] = [];
  for (const host of picked) {
    const key = await loadKey(host.identity_file);
    const jumpKey = await loadKey(host.jump_identity_file);
    rows.push({ host, key_id: key ? key.id : null, jump_key_id: jumpKey ? jumpKey.id : null });
  }
  let report: ImportReport | null = null;
  try {
    report = await invoke<ImportReport>("import_hosts", { rows });
  } catch (e) {
    setImportError(typeof e === "string" ? e : ((e as { message?: string } | null)?.message || String(e)));
  }
  setImportBusy(false);
  if (refreshServers) {
    try { await refreshServers(); } catch { /* */ }
  }
  if (refreshFolders) {
    try { await refreshFolders(); } catch { /* */ }
  }
  if (refreshCredentials) {
    try { await refreshCredentials(); } catch { /* */ }
  }
  if (keysByPath.size > 0 && refreshSshKeys) {
    try { await refreshSshKeys(); } catch { /* */ }
  }
  if (!report) return;
  onImported?.();
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

// AddNodePanel's importSelectedHosts after the move: its own lines up to the
// question about key files, then Fork.importPicked.
const afterImport = async ({ importHosts, importSelected, importBusy, shared, answerConfirm, ...rest }: Parameters<typeof beforeImport>[0]) => {
  if (importSelected.size === 0 || importBusy) return;
  rest.setImportBusy(true);
  rest.setImportError(null);
  const picked = importHosts.filter((h) => importSelected.has(rowKey(h)));
  const keysByPath = new Map<string, ImportedSshKey | null>();
  const keyFiles = [
    ...new Set(picked.flatMap((h) => [h.identity_file, h.jump_identity_file]).filter((p): p is string => !!p)),
  ];
  let withKeys = true;
  if (keyFiles.length > 0) {
    const isShared = await invoke<{ current: string | null }>("share_key_state").then((s) => !!s.current).catch(() => false);
    if (isShared && shared) withKeys = answerConfirm;
  }
  await importPicked({ picked, withKeys, keysByPath, setImportBusy: rest.setImportBusy, setImportError: rest.setImportError, setImportDone: rest.setImportDone, refreshServers: rest.refreshServers, refreshFolders: rest.refreshFolders, refreshCredentials: rest.refreshCredentials, refreshSshKeys: rest.refreshSshKeys, onImported: rest.onImported });
};

const host = (alias: string, more: Partial<ImportedHost> = {}): ImportedHost => ({
  host_alias: alias, hostname: `${alias}.example`, port: 22, user: "root", identity_file: null, proxy_jump: null, ...more,
});

const HOSTS: ImportedHost[] = [
  host("web", { identity_file: "~/.ssh/id_ed25519", key: "Prod/web", folder: ["Prod"] }),
  host("web", { identity_file: "~/.ssh/id_ed25519", key: "Test/web", folder: ["Test"], jump_identity_file: "~/.ssh/jump.ppk", proxy_jump: "bastion" }),
  host("db", { identity_file: "~/.ssh/locked", credential: "dba", run_on_connect: "sudo -i" }),
  host("desk", { rdp: true, port: 3389 }),
  host("sw1", { telnet: true, port: 23 }),
  host("left-out"),
];

const REPORT: ImportReport = {
  added: 4, already_there: 1, folders_created: 2, logins_created: ["dba"], jump_hosts_created: ["bastion"], jump_linked: 1,
  multi_hop: ["a", "b", "c", "d", "e", "f"], rdp_added: 1, rdp_skipped: ["vnc-box: VNC"],
};

/** One run of an import, as the backend and the panel saw it. */
const run = async (impl: typeof beforeImport, scenario: { shared: boolean; answerConfirm: boolean; importFails?: boolean; selected: string[] }) => {
  calls.length = 0;
  const seen: unknown[][] = [];
  answer = (cmd, args) => {
    if (cmd === "share_key_state") return { current: scenario.shared ? "fp" : null };
    if (cmd === "import_ssh_key_file") {
      const path = (args as { path: string }).path;
      if (path.endsWith(".ppk")) return new Error("unsupported key format");
      return { id: path.length, name: path, reused: false, encrypted: path.endsWith("locked") };
    }
    if (cmd === "import_hosts") return scenario.importFails ? new Error("database is locked") : REPORT;
    return undefined;
  };
  const rec = (name: string) => (...a: unknown[]) => { seen.push([name, ...a]); };
  await impl({
    importHosts: HOSTS, importSelected: new Set(scenario.selected), importBusy: false, shared: scenario.shared, answerConfirm: scenario.answerConfirm,
    setImportBusy: rec("busy"), setImportError: rec("error"), setImportDone: rec("done"),
    refreshServers: rec("servers"), refreshFolders: rec("folders"), refreshCredentials: rec("credentials"), refreshSshKeys: rec("keys"),
    onImported: rec("imported"),
  });
  return { calls: [...calls], seen };
};

describe("importing sessions", () => {
  beforeEach(() => { calls.length = 0; });

  const scenarios = [
    { shared: false, answerConfirm: false, selected: HOSTS.map(rowKey) },
    { shared: true, answerConfirm: true, selected: HOSTS.map(rowKey) },
    { shared: true, answerConfirm: false, selected: HOSTS.map(rowKey) },
    { shared: false, answerConfirm: false, selected: ["Test/web", "desk"] },
    { shared: false, answerConfirm: false, importFails: true, selected: ["Prod/web", "db"] },
    { shared: false, answerConfirm: false, selected: ["sw1"] },
    { shared: false, answerConfirm: false, selected: [] },
  ];

  for (const s of scenarios) {
    it(`sends the same rows and reports the same: ${JSON.stringify(s)}`, async () => {
      const before = await run(beforeImport, s);
      const after = await run(afterImport, s);
      expect(after.calls).toEqual(before.calls);
      expect(after.seen).toEqual(before.seen);
    });
  }

  it("sends each picked host with its key ids, in the order shown", async () => {
    const { calls: sent, seen } = await run(afterImport, scenarios[0]);
    const rows = (JSON.parse(sent.find((c) => c.cmd === "import_hosts")!.args) as { rows: ImportRow[] }).rows;
    expect(rows.map((r) => [rowKey(r.host), r.key_id, r.jump_key_id])).toEqual([
      ["Prod/web", 17, null], ["Test/web", 17, null], ["db", 13, null], ["desk", null, null], ["sw1", null, null], ["left-out", null, null],
    ]);
    // The same key file is read once.
    expect(sent.filter((c) => c.cmd === "import_ssh_key_file").map((c) => (JSON.parse(c.args) as { path: string }).path))
      .toEqual(["~/.ssh/id_ed25519", "~/.ssh/jump.ppk", "~/.ssh/locked"]);
    expect(seen.map((s) => s[0])).toEqual(["busy", "error", "busy", "servers", "folders", "credentials", "keys", "imported", "done"]);
  });
});

describe("importing FileZilla's sites", () => {
  const sites = [
    host("files", { protocol: "ftps", port: 21, password: "s3cr3t" }),
    host("sftp", { identity_file: "~/.ssh/id_ed25519", password: "unused" }),
    host("open", { protocol: "ftp", port: 21 }),
  ];
  const rowsSent = async (withPasswords: boolean | undefined) => {
    calls.length = 0;
    answer = (cmd) => cmd === "import_ssh_key_file" ? { id: 3, name: "k", reused: false, encrypted: false } : cmd === "import_hosts" ? REPORT : undefined;
    const quiet = () => {};
    await importPicked({ picked: sites, withKeys: true, ...(withPasswords === undefined ? {} : { withPasswords }), keysByPath: new Map(), setImportBusy: quiet, setImportError: quiet, setImportDone: quiet });
    return (JSON.parse(calls.find((c) => c.cmd === "import_hosts")!.args) as { rows: ImportRow[] }).rows;
  };

  it("sends an FTP site's kind, and its password unless told not to or beside a key", async () => {
    for (const allowed of [undefined, true]) {
      const rows = await rowsSent(allowed);
      expect(rows.map((r) => [r.host.host_alias, r.host.protocol, r.host.password])).toEqual([
        ["files", "ftps", "s3cr3t"], ["sftp", undefined, undefined], ["open", "ftp", undefined],
      ]);
    }
    const without = await rowsSent(false);
    expect(without.map((r) => [r.host.protocol, r.host.password])).toEqual([["ftps", undefined], [undefined, undefined], ["ftp", undefined]]);
    expect(without[1].key_id).toBe(3);
  });
});

describe("parsing an export", () => {
  it("sets the hosts, then the line about what it can't import", async () => {
    answer = () => ({ hosts: [host("a")], skipped: "1 VNC session can't be imported." });
    const order: unknown[] = [];
    await parseClientImport("text", (h) => order.push(["hosts", h]), (s) => order.push(["skipped", s]));
    expect(order).toEqual([["hosts", [host("a")]], ["skipped", "1 VNC session can't be imported."]]);
    expect(calls.at(-1)).toEqual({ cmd: "parse_client_import", args: '{"text":"text"}' });
  });

  it("parses a chosen file as if it had been pasted, and shows why it couldn't", async () => {
    const order: unknown[] = [];
    answer = () => "[MobaXterm]";
    await chooseImportFile((e) => order.push(["error", e]), (t) => order.push(["text", t]), async (t) => { order.push(["parse", t]); });
    answer = () => null;
    await chooseImportFile((e) => order.push(["error", e]), (t) => order.push(["text", t]), async (t) => { order.push(["parse", t]); });
    answer = () => new Error("denied");
    await chooseImportFile((e) => order.push(["error", e]), (t) => order.push(["text", t]), async (t) => { order.push(["parse", t]); });
    expect(order).toEqual([
      ["error", null], ["text", "[MobaXterm]"], ["parse", "[MobaXterm]"],
      ["error", null],
      ["error", null], ["error", "denied"],
    ]);
  });
});

describe("deleting a folder from the grid", () => {
  // NodeGrid's question before the move.
  const before = (f: { name: string }, inside: Set<number>, servers: { id: number; name: string; folder_id?: number | null; jump_host_id?: number | null }[]) => {
    const gone = new Set<number>((servers || []).filter((x) => x.folder_id != null && inside.has(x.folder_id)).map((x) => x.id));
    const note = jumpHostNote(servers, gone);
    return `“${f.name}” and every folder and server inside it will be removed.${note}`;
  };
  const servers = [
    { id: 1, name: "bastion", folder_id: 5 },
    { id: 2, name: "app", folder_id: null, jump_host_id: 1 },
    { id: 3, name: "db", folder_id: 6, jump_host_id: 1 },
    { id: 4, name: "solo", folder_id: 9 },
  ];
  it("asks the same", () => {
    for (const inside of [new Set([5]), new Set([5, 6]), new Set([9]), new Set<number>()]) {
      expect(deleteFolderMessage({ name: "Prod" }, inside, servers)).toBe(before({ name: "Prod" }, inside, servers));
    }
    expect(deleteFolderMessage({ name: "Prod" }, new Set([5]), servers)).toContain("jump host");
  });
});
