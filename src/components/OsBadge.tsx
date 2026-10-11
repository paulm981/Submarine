// The badge for the OS a server last reported (src-tauri/src/server_os.rs):
// two letters on the distribution's colour, so servers tell apart at a
// glance in the session tree and the server grid. Unknown → null, and the
// caller shows its usual icon.

const OS: Record<string, { label: string; name: string; bg: string; fg?: string }> = {
  ubuntu: { label: "Ub", name: "Ubuntu", bg: "#E95420" },
  debian: { label: "Db", name: "Debian", bg: "#A81D33" },
  raspbian: { label: "Pi", name: "Raspberry Pi OS", bg: "#C51A4A" },
  linuxmint: { label: "Mi", name: "Linux Mint", bg: "#4AA02C" },
  rhel: { label: "RH", name: "Red Hat Enterprise Linux", bg: "#CC0000" },
  centos: { label: "Ce", name: "CentOS", bg: "#932279" },
  rocky: { label: "Ro", name: "Rocky Linux", bg: "#10B981", fg: "#04261a" },
  almalinux: { label: "Al", name: "AlmaLinux", bg: "#0F4266" },
  fedora: { label: "Fe", name: "Fedora", bg: "#3C6EB4" },
  ol: { label: "OL", name: "Oracle Linux", bg: "#C74634" },
  amzn: { label: "AL", name: "Amazon Linux", bg: "#FF9900", fg: "#1f1300" },
  sles: { label: "Su", name: "SUSE Linux Enterprise", bg: "#30BA78", fg: "#06231a" },
  opensuse: { label: "oS", name: "openSUSE", bg: "#73BA25", fg: "#132006" },
  "opensuse-leap": { label: "oS", name: "openSUSE Leap", bg: "#73BA25", fg: "#132006" },
  alpine: { label: "Ap", name: "Alpine Linux", bg: "#0D597F" },
  arch: { label: "Ar", name: "Arch Linux", bg: "#1793D1" },
  freebsd: { label: "BS", name: "FreeBSD", bg: "#AB2B28" },
  macos: { label: "Mc", name: "macOS", bg: "#6B7280" },
  windows: { label: "Wi", name: "Windows", bg: "#0078D4" },
  linux: { label: "Lx", name: "Linux", bg: "#52525B" },
};

// Own keys only: the ID comes from the server, and `constructor` or
// `__proto__` would otherwise find Object's.
const lookup = (os: string) => (Object.hasOwn(OS, os) ? OS[os] : undefined);

export const osName = (os?: string | null): string | null => (os ? lookup(os)?.name ?? os : null);

export const OsBadge = ({ os, size = 14 }: { os?: string | null; size?: number }) => {
  if (!os) return null;
  const known = lookup(os) ?? { label: os.slice(0, 2).replace(/^./, (c) => c.toUpperCase()), name: os, bg: "#52525B" };
  return (
    <span
      title={known.name}
      className="shrink-0 inline-flex items-center justify-center rounded font-black leading-none select-none"
      style={{ width: size, height: size, fontSize: Math.round(size * 0.5), background: known.bg, color: known.fg ?? "#fff" }}
    >
      {known.label}
    </span>
  );
};
