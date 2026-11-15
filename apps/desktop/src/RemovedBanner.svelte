<script lang="ts">
  import { Alert, Button } from "@figlock/ui";

  import { fmtDay } from "./util";

  type Props = {
    /** When the vault removed this Mac, in Unix milliseconds. */
    removedAtMs: number;
    /** Go back to the screen with the ways forward. */
    onoptions: () => void;
  };
  let { removedAtMs, onoptions }: Props = $props();

  const date = $derived(fmtDay(removedAtMs));
</script>

<div class="border-b border-border bg-surface px-4 py-2" data-testid="removed-banner">
  <div class="mx-auto flex max-w-190 flex-wrap items-center gap-3">
    <div class="min-w-0 flex-1">
      <Alert tone="warning">
        This Mac was removed from the vault on {date}. Changes made here are no longer accepted.
        Your copy stays readable.
      </Alert>
    </div>
    <Button onclick={onoptions}>See options</Button>
  </div>
</div>
