<script lang="ts">
  import { Button } from "@figlock/ui";

  import Shell from "./Shell.svelte";

  type Props = { onpick: (view: "create" | "open" | "join") => void };
  let { onpick }: Props = $props();
</script>

<Shell
  title="Welcome to FigLock"
  subtitle="Your passwords and secrets, encrypted and kept in storage you own."
>
  <div class="flex flex-col gap-2">
    <Button variant="primary" block onclick={() => onpick("create")}>Create a vault</Button>
    <Button block onclick={() => onpick("open")}>Open an existing vault</Button>
    <Button block onclick={() => onpick("join")}>Add this Mac with an invite code</Button>
  </div>
  <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
    FigLock has no account and no server. Your vault is one encrypted folder that only your master
    password and Secret Key can open.
  </p>
</Shell>
