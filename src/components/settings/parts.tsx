// The pieces the fork's settings cards are made of, in the original
// project's look: a card under a heading, a switch, and Learn more.
import { useId, useState, type ReactNode } from "react";
import { ChevronDown, type LucideIcon } from "lucide-react";

/** A card under its heading, as the original project's cards are. */
export function SettingsSection({ icon: Icon, title, children }: {
  icon: LucideIcon;
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
      <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
        <Icon size={14} /> {title}
      </div>
      {children}
    </section>
  );
}

export const CARD = "bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl";

export function Switch({ on, label, onToggle, disabled }: {
  on: boolean;
  label: string;
  onToggle: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={onToggle}
      className={`relative shrink-0 w-11 h-6 rounded-full transition-colors disabled:opacity-40 ${on ? "bg-primary" : "bg-zinc-700"}`}
    >
      <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${on ? "left-[22px]" : "left-0.5"}`} />
    </button>
  );
}

/** A setting with a switch: its name, one line on what it does, and the
 *  rest behind Learn more. */
export function SwitchRow({ title, line, more, on, onToggle, disabled, small, error }: {
  title: string;
  line: ReactNode;
  more?: ReactNode;
  on: boolean;
  onToggle: () => void;
  disabled?: boolean;
  small?: boolean;
  error?: string | null;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className={small ? "text-[12.5px] font-semibold text-zinc-100" : "text-sm font-semibold text-zinc-100"}>{title}</div>
        <p className={small ? "text-[11.5px] text-zinc-500 leading-relaxed mt-0.5" : "text-[12px] text-zinc-400 leading-relaxed mt-1"}>{line}</p>
        {more && <LearnMore>{more}</LearnMore>}
        {error && <p className="text-[11px] text-red-400 mt-1">{error}</p>}
      </div>
      <Switch on={on} label={title} onToggle={onToggle} disabled={disabled} />
    </div>
  );
}

/** The longer explanation of a setting, folded away until asked for: it
 *  opens in place, under its button. */
export function LearnMore({ children, label = "Learn more" }: { children: ReactNode; label?: string }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <div className="mt-1">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((v) => !v)}
        className="inline-flex items-center gap-1 text-[11.5px] font-semibold text-zinc-400 hover:text-white transition-colors"
      >
        <ChevronDown size={12} className={`transition-transform ${open ? "rotate-180" : ""}`} />
        {open ? "Show less" : label}
      </button>
      <div id={id} hidden={!open} className="mt-1.5 space-y-2 text-[11.5px] text-zinc-500 leading-relaxed">
        {children}
      </div>
    </div>
  );
}
