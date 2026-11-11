<script lang="ts">
  type Props = { label: string; value: string; options: { value: string; label: string }[] };
  let { label, value = $bindable(), options }: Props = $props();

  const id = $props.id();
</script>

<div>
  <label for={id} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">{label}</label>
  <select
    {id}
    bind:value
    class="h-(--control-height-md) min-w-56 rounded-(--radius-sm) border border-border-strong bg-surface px-3 text-text"
  >
    {#each options as o (o.value)}
      <option value={o.value}>{o.label}</option>
    {/each}
  </select>
</div>
