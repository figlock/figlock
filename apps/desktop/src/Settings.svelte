<script lang="ts">
  import AboutPage from "./AboutPage.svelte";
  import AppearanceRow from "./AppearanceRow.svelte";
  import AutoLockPanel from "./AutoLockPanel.svelte";
  import CommandLinePanel from "./CommandLinePanel.svelte";
  import CredentialsPanel from "./CredentialsPanel.svelte";
  import Devices from "./Devices.svelte";
  import ExportPanel from "./ExportPanel.svelte";
  import ImportPanel from "./ImportPanel.svelte";
  import RecoveryPanel from "./RecoveryPanel.svelte";
  import SecretKeyPanel from "./SecretKeyPanel.svelte";
  import SettingsGroup from "./SettingsGroup.svelte";
  import SettingsPage from "./SettingsPage.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SyncSettings from "./SyncSettings.svelte";
  import TouchIdPanel from "./TouchIdPanel.svelte";
  import UpdatePanel from "./UpdatePanel.svelte";
  import { syncValue } from "./sync";
  import { updates } from "./updates.svelte";
  import type { VaultState } from "./vault";
  import { getWorkspace } from "./workspace";

  const { nav, sync } = getWorkspace();
  const page = $derived(nav.current.p === "settings" ? (nav.current.page ?? null) : null);

  /** The newest vault state a panel got back, handed to the others so none shows a stale line. */
  let latest = $state<VaultState | null>(null);

  const updateValue = $derived(
    updates.view?.state === "ready"
      ? "Restart to update"
      : updates.view?.state === "available"
        ? "Update available"
        : "",
  );
</script>

{#if page === "security"}
  <SettingsPage title="Security">
    <TouchIdPanel {latest} onstate={(s) => (latest = s)} />
    <CredentialsPanel />
    <SecretKeyPanel {latest} onstate={(s) => (latest = s)} />
    <RecoveryPanel />
  </SettingsPage>
{:else if page === "sync"}
  <SettingsPage title="Sync"><SyncSettings /></SettingsPage>
{:else if page === "devices"}
  <SettingsPage title="Devices"><Devices /></SettingsPage>
{:else if page === "updates"}
  <SettingsPage title="Updates"><UpdatePanel /></SettingsPage>
{:else if page === "advanced"}
  <SettingsPage title="Command line tool"><CommandLinePanel /></SettingsPage>
{:else if page === "export"}
  <SettingsPage title="Export"><ExportPanel /></SettingsPage>
{:else if page === "import"}
  <SettingsPage title="Import"><ImportPanel /></SettingsPage>
{:else if page === "about"}
  <AboutPage />
{:else}
  <div class="grid gap-6">
    <h2 id="page-title" tabindex="-1" class="text-(length:--text-md) leading-6 font-semibold">
      Settings
    </h2>

    <SettingsGroup title="General">
      <AppearanceRow />
      <AutoLockPanel />
      <SettingsRow
        icon="download"
        title="Updates"
        testid="settings-updates"
        value={updateValue}
        onclick={() => nav.go({ p: "settings", page: "updates" })}
      />
    </SettingsGroup>

    <SettingsGroup title="This vault">
      <SettingsRow
        icon="shield-check"
        title="Security"
        testid="settings-security"
        hint="Touch ID, password, Secret Key, Recovery Key"
        onclick={() => nav.go({ p: "settings", page: "security" })}
      />
      <SettingsRow
        icon="cloud"
        title="Sync"
        testid="settings-sync"
        value={syncValue(sync.view)}
        onclick={() => nav.go({ p: "settings", page: "sync" })}
      />
      <SettingsRow
        icon="laptop"
        title="Devices"
        testid="settings-devices"
        onclick={() => nav.go({ p: "settings", page: "devices" })}
      />
      <SettingsRow
        icon="upload"
        title="Import"
        testid="settings-import"
        hint="Bring items from Chrome, Apple Passwords or notes"
        onclick={() => nav.go({ p: "settings", page: "import" })}
      />
      <SettingsRow
        icon="download"
        title="Export"
        testid="settings-export"
        hint="Save your items as a JSON file"
        onclick={() => nav.go({ p: "settings", page: "export" })}
      />
    </SettingsGroup>

    <SettingsGroup title="More">
      <SettingsRow
        icon="terminal"
        title="Command line tool"
        testid="settings-advanced"
        onclick={() => nav.go({ p: "settings", page: "advanced" })}
      />
      <SettingsRow
        icon="info"
        title="About FigLock"
        testid="settings-about"
        onclick={() => nav.go({ p: "settings", page: "about" })}
      />
    </SettingsGroup>
  </div>
{/if}
