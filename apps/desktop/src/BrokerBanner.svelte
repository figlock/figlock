<script lang="ts">
  import { Alert, Button } from "@figlock/ui";
  import { onMount } from "svelte";

  import { brokerStatus, brokerTakeOver, errorOf } from "./vault";

  /** Shown while a broker started in a terminal answers agents instead of this window (ADR-0040). */
  let terminal = $state(false);
  let busy = $state(false);
  let error = $state("");

  async function check() {
    try {
      terminal = (await brokerStatus()) === "cli";
    } catch {
      // The banner is an extra: a window that cannot ask shows none.
    }
  }

  onMount(() => {
    void check();
    const timer = setInterval(() => void check(), 30_000);
    return () => clearInterval(timer);
  });

  async function takeOver() {
    busy = true;
    error = "";
    try {
      terminal = (await brokerTakeOver()) === "cli";
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }
</script>

{#if terminal}
  <div class="border-b border-border bg-surface px-4 py-2" data-testid="broker-banner">
    <div class="mx-auto flex max-w-190 flex-wrap items-center gap-3">
      <div class="min-w-0 flex-1">
        <Alert tone="warning">
          {#if error}{error}{:else}
            A broker started in a terminal is answering agents. Let FigLock do it instead and agents
            will ask here. Approvals already given by that broker end.
          {/if}
        </Alert>
      </div>
      <Button onclick={takeOver} disabled={busy}>
        {busy ? "Stopping..." : "Use FigLock for agents"}
      </Button>
    </div>
  </div>
{/if}
