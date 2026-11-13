<script lang="ts">
  import { onMount } from "svelte";

  import Create from "./Create.svelte";
  import Forgot from "./Forgot.svelte";
  import NewKitDialog from "./NewKitDialog.svelte";
  import Join from "./Join.svelte";
  import OpenExisting from "./OpenExisting.svelte";
  import Unlock from "./Unlock.svelte";
  import Welcome from "./Welcome.svelte";
  import Workspace from "./Workspace.svelte";
  import { activity, lock, onAutoLock, vaultState, type VaultState } from "./vault";

  let vault = $state<VaultState | null>(null);
  let failed = $state(false);
  let notice = $state("");
  let view = $state<"welcome" | "create" | "open" | "join" | "forgot">("welcome");

  const refresh = () =>
    vaultState().then(
      (s) => (vault = s),
      () => (failed = true),
    );
  const enter = (s: VaultState) => {
    vault = s;
    notice = "";
    view = "welcome";
  };

  onMount(() => {
    refresh();
    // The idle timer runs in the app core; tell it about input, at most once a second.
    let last = 0;
    const seen = () => {
      const now = Date.now();
      if (vault?.phase === "unlocked" && now - last > 1000) {
        last = now;
        activity().catch(() => {});
      }
    };
    const events = ["keydown", "pointerdown", "pointermove", "wheel"] as const;
    for (const e of events) window.addEventListener(e, seen, { passive: true });
    const off = onAutoLock(refresh);
    return () => {
      for (const e of events) window.removeEventListener(e, seen);
      off.then((f) => f());
    };
  });

  async function lockNow() {
    await lock();
    await refresh();
  }
</script>

{#if failed}
  <main class="grid min-h-screen place-items-center p-6">
    <p class="text-text-secondary">Can't reach the app core. Restart FigLock and try again.</p>
  </main>
{:else if vault === null}
  <main class="grid min-h-screen place-items-center p-6">
    <p class="text-text-secondary">Starting...</p>
  </main>
{:else if vault.phase === "unlocked"}
  <Workspace
    onlock={lockNow}
    onlocked={(message) => {
      notice = message ?? "";
      void refresh();
    }}
  />
  {#if vault.pendingKit}
    <NewKitDialog
      secretKey={vault.pendingKit}
      ondone={() => {
        if (vault) vault = { ...vault, pendingKit: null };
      }}
    />
  {/if}
{:else if vault.phase === "locked"}
  <Unlock {vault} {notice} ondone={enter} />
{:else if view === "create"}
  <Create touchAvailable={vault.touchAvailable} onback={() => (view = "welcome")} ondone={enter} />
{:else if view === "open"}
  <OpenExisting
    touchAvailable={vault.touchAvailable}
    onback={() => (view = "welcome")}
    ondone={enter}
  />
{:else if view === "join"}
  <Join touchAvailable={vault.touchAvailable} onback={() => (view = "welcome")} ondone={enter} />
{:else if view === "forgot"}
  <Forgot touchAvailable={vault.touchAvailable} onback={() => (view = "welcome")} ondone={enter} />
{:else}
  <Welcome onpick={(v) => (view = v)} />
{/if}
