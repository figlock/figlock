<script lang="ts">
  type Props = { checked: boolean; disabled?: boolean };
  let { checked = $bindable(), disabled = false }: Props = $props();
</script>

<label class="flex items-start gap-2 text-(length:--text-sm) leading-5">
  <input type="checkbox" bind:checked {disabled} class="mt-0.5 size-4 accent-primary" />
  <span>
    Unlock this Mac with Touch ID
    <span class="block text-(length:--text-xs) text-text-tertiary">
      Open FigLock with your fingerprint instead of typing your master password. This Mac keeps your
      Secret Key either way, so the master password is all you ever type.
    </span>
  </span>
</label>
