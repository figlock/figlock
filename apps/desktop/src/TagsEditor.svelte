<script lang="ts">
  import { Icon } from "@figlock/ui";

  type Props = { tags: string[] };
  let { tags = $bindable() }: Props = $props();

  const id = $props.id();
  let typed = $state("");

  /** Lowercase letters, digits, hyphen and underscore, up to 40 characters (the schema). */
  const clean = (s: string) =>
    s
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]/g, "")
      .slice(0, 40);

  function commit() {
    const tag = clean(typed);
    typed = "";
    if (tag && !tags.includes(tag)) tags = [...tags, tag];
  }
</script>

<div>
  <label for={id} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">Tags</label>
  <div
    class="flex min-h-(--control-height-md) flex-wrap items-center gap-1.5 rounded-(--radius-sm) border border-border-strong bg-surface px-2 py-1 focus-within:outline-2 focus-within:outline-focus"
  >
    {#each tags as tag (tag)}
      <span
        class="inline-flex h-6 items-center gap-0.5 rounded-(--radius-xs) bg-surface-sunken pr-0.5 pl-2 text-(length:--text-xs) font-medium"
      >
        {tag}
        <button
          type="button"
          onclick={() => (tags = tags.filter((t) => t !== tag))}
          class="grid size-5 place-items-center rounded-sm text-text-secondary hover:bg-border"
        >
          <Icon name="x" size={12} /><span class="sr-only">Remove tag {tag}</span>
        </button>
      </span>
    {/each}
    <input
      {id}
      bind:value={typed}
      maxlength="40"
      aria-describedby="{id}-help"
      autocomplete="off"
      spellcheck="false"
      onkeydown={(e) => {
        if (e.key === "Enter" || e.key === ",") {
          e.preventDefault();
          commit();
        } else if (e.key === "Backspace" && !typed && tags.length) {
          tags = tags.slice(0, -1);
        }
      }}
      onblur={commit}
      class="h-6 min-w-24 flex-1 bg-transparent outline-none"
    />
  </div>
  <p id="{id}-help" class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">
    Lowercase letters, digits, hyphen and underscore, up to 40 characters. Press Return or comma to
    add.
  </p>
</div>
