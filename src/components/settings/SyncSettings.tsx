// Settings › Sync: beside the original project's Cloud Sync card (auto-sync),
// the switch for Submarine Cloud itself.
import { useEffect, useState } from "react";
import { Cloud } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { CARD, SettingsSection, SwitchRow } from "./parts";

// Submarine Cloud: the original project's hosted sync, sign-in and sharing.
// Kept by the backend (cloud.rs), which contacts it only while this is on;
// off by default, as profiles sync through the user's own storage instead.
function CloudSwitch() {
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<boolean>("get_cloud_enabled").then(setOn).catch(() => setOn(null));
  }, []);
  const toggle = () => {
    if (on === null) return;
    setError(null);
    invoke<boolean>("set_cloud_enabled", { enabled: !on }).then(setOn).catch((e) => setError(String(e)));
  };
  return (
    <SwitchRow
      title="Submarine Cloud"
      line="The original project's hosted sync, with sign-in and sharing. Off, Submarine never contacts it."
      more={
        <p>
          The service is at submarine.sinaxhpm.com. While this is off, its buttons are hidden too, and profiles sync
          through your own folder or git repository instead, set up on the Profile tab. Auto-sync, in the Cloud Sync card, works for both.
        </p>
      }
      on={on === true}
      disabled={on === null}
      onToggle={toggle}
      error={error}
    />
  );
}

export function SubmarineCloudSection() {
  return (
    <SettingsSection icon={Cloud} title="Submarine Cloud">
      <div className={CARD}>
        <CloudSwitch />
      </div>
    </SettingsSection>
  );
}
