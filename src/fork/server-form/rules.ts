// ─────────────────────────────────────────────────────────────────────────────
// The fork's rules for the add/edit server form (AddNodePanel), as plain types
// and functions that AddNodePanel and the rest of src/fork/server-form use
// (#163). AddNodePanel.tsx is the original project's file, so every line the
// fork adds there can conflict at the next sync. Each returns what
// AddNodePanel had in their place.
// ─────────────────────────────────────────────────────────────────────────────

import type { FtpProtocol, ServerProtocol } from "../../util/serverProtocol";

/** An SSH or RDP password held while the server is switched to telnet or
 *  VNC (protocolSwitch), to come back if it's switched out again. */
export type HeldPassword = { from: string; password: string; dirty: boolean };

/** What the fork reads and writes of the form's `newNode` (DesktopApp's). */
export type ServerFormNode = {
  id?: number;
  protocol?: string;
  authType?: string;
  credentialId?: unknown;
  folderId?: string;
  jumpHostId?: string;
  password?: string;
  password_dirty?: boolean;
  heldPassword?: HeldPassword;
  terminalTheme?: string;
  highlight?: string;
};

/** What the fork adds to the original project's ImportedHost (the shape
 *  `parse_ssh_config` returns), for what `parse_client_import` reads from
 *  MobaXterm and PuTTY. */
export type ImportedHostExtra = {
  // Unique within one parse, as names can repeat: MobaXterm's folder and
  // name, else the row's place in the list. Empty means host_alias is.
  key?: string;
  folder?: string[];
  // MobaXterm saved login (`[name]` as the user): becomes a shared Login.
  credential?: string | null;
  run_on_connect?: string | null;
  jump_identity_file?: string | null;
  // The jump host's MobaXterm saved login, if it signs in with one.
  jump_credential?: string | null;
  // A Remote Desktop session: becomes an RDP server.
  rdp?: boolean;
  // A PuTTY telnet session: becomes a telnet server.
  telnet?: boolean;
};

/** AddNodePanel's ImportedHost, with the fork's part. */
export type ImportedHost = {
  host_alias: string;
  hostname: string;
  port: number;
  user: string;
  identity_file: string | null;
  proxy_jump: string | null;
  // FileZilla's site list: an FTP site's kind, and the password it kept.
  protocol?: FtpProtocol;
  password?: string;
} & ImportedHostExtra;

// What `parse_client_import` hands back: the hosts, and a line about the
// sessions it can't import (MobaXterm VNC, serial…).
export type ClientImport = { hosts: ImportedHost[]; skipped: string | null };

// One picked host for `import_hosts`, with the vault ids of its keys.
export type ImportRow = { host: ImportedHost; key_id: number | null; jump_key_id: number | null };

// What `import_hosts` reports back (src-tauri/src/host_import.rs).
export type ImportReport = {
  added: number;
  already_there: number;
  folders_created: number;
  logins_created: string[];
  jump_hosts_created: string[];
  jump_linked: number;
  multi_hop: string[];
  rdp_added: number;
  rdp_skipped: string[];
};

// What `import_ssh_key_file` hands back after putting a key file in the vault
// (AddNodePanel's ImportedSshKey).
export type ImportedSshKey = {
  id: number;
  name: string;
  reused: boolean;
  encrypted: boolean;
};

/** An import row's key in the picked set, in place of its host_alias. */
export const rowKey = (h: ImportedHost) => h.key || h.host_alias;

export const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** An error's text, as the form shows it. */
export const errText = (e: unknown): string =>
  typeof e === "string" ? e : ((e as { message?: string } | null | undefined)?.message || String(e));

/** What a switch of the server's kind changes besides the protocol and port,
 *  spread after them in setProtocol (in AddNodePanel's place of its
 *  `authType`).
 *
 *  No SSH key outside SSH, and no saved login for VNC: VNC sends its
 *  password with weak protection and doesn't check the server's identity,
 *  so a shared login's password would be open to anyone on the path. */
export const protocolSwitch = (newNode: ServerFormNode, protocol: ServerProtocol) => {
  const dropsLogin = (protocol !== "ssh" && newNode.authType === "custom_key") || (protocol === "vnc" && newNode.authType === "vault");
  const authType = dropsLogin ? "custom_pass" : newNode.authType;
  const credentialId = protocol === "vnc" ? "" : newNode.credentialId;
  // An SSH or RDP password isn't sent over VNC (weakly protected) or
  // telnet (not protected at all): switching into either from another kind
  // empties the field, and the save clears the saved one too. It's held
  // here, so switching out again before saving (to SSH or RDP) brings it
  // back as it was.
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
  return { authType, credentialId, ...password };
};

/** A new folder made from the form goes inside the folder picked above, so
 *  folders can nest; at the top level when that's Root. */
export const folderParentId = (newNode: ServerFormNode) => (newNode.folderId ? parseInt(newNode.folderId) : null);
