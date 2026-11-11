<script lang="ts">
  import { Button } from "@figlock/ui";

  import Shell from "./Shell.svelte";

  type Props = { onlock: () => void };
  let { onlock }: Props = $props();
</script>

<Shell title="Your vault is unlocked" subtitle="Your items will appear here.">
  <Button block onclick={onlock}>Lock</Button>
</Shell>
