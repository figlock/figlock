<script lang="ts">
  import { Button, TextField } from "@figlock/ui";

  import { chooseFolder } from "./vault";

  type Props = { value: string; help?: string; error?: string; label?: string };
  let { value = $bindable(), help, error, label = "Vault folder" }: Props = $props();

  async function choose() {
    const picked = await chooseFolder();
    if (picked) value = picked;
  }
</script>

<TextField {label} bind:value {help} {error} readonly mono>
  {#snippet trailing()}
    <Button onclick={choose}>Choose...</Button>
  {/snippet}
</TextField>
