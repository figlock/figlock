<script lang="ts">
  import Generator from "./Generator.svelte";
</script>

<div class="mx-auto grid max-w-xl gap-4 p-6">
  <div>
    <h2 id="page-title" tabindex="-1" class="text-(length:--text-md) leading-6 font-semibold">
      Generator
    </h2>
    <p class="text-text-secondary">
      Make a strong password or passphrase. Nothing is saved until you use it in an item.
    </p>
  </div>
  <Generator />
</div>
