// The fork's own cards, under the tab each belongs to. The original
// project's cards stay in SettingsPanel.tsx, sorted by `InTab`; these come
// after them, in this order.
import { IS_MOBILE } from "../../util/platform";
import type { SettingsTab } from "./categories";
import { AiToolsSection } from "./AiToolsSettings";
import { LiveEditSection } from "./GeneralSettings";
import { LicensesSection, TroubleshootingSection, UpdatesSection } from "./HelpSettings";
import { ServersSection, SessionsSection } from "./ServersSettings";
import { SubmarineCloudSection } from "./SyncSettings";
import { HomeTerminalSection } from "./TerminalSettings";

export function ForkSettings({ tab, settings, setSettings }: { tab: SettingsTab; settings: any; setSettings: any }) {
  switch (tab) {
    case "general":
      // Not on a phone or tablet, which has no editor to open files in.
      return IS_MOBILE ? null : <LiveEditSection />;
    case "terminal":
      return IS_MOBILE ? null : <HomeTerminalSection />;
    case "servers":
      return (
        <>
          <ServersSection settings={settings} setSettings={setSettings} />
          <SessionsSection />
        </>
      );
    case "ai":
      return IS_MOBILE ? null : <AiToolsSection />;
    case "sync":
      return <SubmarineCloudSection />;
    case "help":
      return (
        <>
          <UpdatesSection settings={settings} setSettings={setSettings} />
          <TroubleshootingSection />
          <LicensesSection />
        </>
      );
  }
}
