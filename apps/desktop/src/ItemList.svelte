<script lang="ts">
  import { Button, Icon } from "@figlock/ui";

  import { kindInfo, type ItemSummary } from "./items";
  import type { Filter } from "./nav.svelte";
  import { plural } from "./util";
  import { getWorkspace } from "./workspace";

  type Props = { filter: Filter };
  let { filter }: Props = $props();

  const { nav, library } = getWorkspace();

  const searching = $derived(library.query.trim() !== "");
  const source = $derived(filter === "deleted" ? library.deleted : library.hits);
  const rows = $derived(
    filter === "all" || filter === "deleted" ? source : source.filter((i) => i.kind === filter),
  );
  const name = $derived(
    filter === "all"
      ? "items"
      : filter === "deleted"
        ? "deleted items"
        : kindInfo(filter).plural.toLowerCase(),
  );
  const vaultEmpty = $derived(
    library.loaded && library.all.length === 0 && library.deleted.length === 0,
  );

  const open = (item: ItemSummary) => nav.go({ p: "item", id: item.id, from: filter });
  const add = () =>
    nav.go(
      filter !== "all" && filter !== "deleted"
        ? { p: "edit", kind: filter, from: filter }
        : { p: "pick", from: filter },
    );
</script>

{#if !library.loaded}
  <p class="px-2 text-text-secondary">Loading your items...</p>
{:else if vaultEmpty && !searching}
  <div class="flex flex-col items-center gap-3 px-6 py-16 text-center text-text-secondary">
    <div class="grid size-12 place-items-center rounded-full bg-surface-sunken">
      <Icon name="layers" size={24} />
    </div>
    <h2
      class="text-(length:--text-md) leading-6 font-semibold text-text"
      id="page-title"
      tabindex="-1"
    >
      Your vault is empty
    </h2>
    <p class="max-w-95">
      Add a login, API token, note or document. FigLock encrypts every item on this device before it
      syncs.
    </p>
    <Button variant="primary" onclick={() => nav.go({ p: "pick", from: "all" })}>
      <Icon name="plus" />New item
    </Button>
  </div>
{:else if rows.length === 0}
  <div class="flex flex-col items-center gap-3 px-6 py-16 text-center text-text-secondary">
    <div class="grid size-12 place-items-center rounded-full bg-surface-sunken">
      <Icon name={filter === "deleted" ? "trash" : "search"} size={20} />
    </div>
    <h2
      class="text-(length:--text-md) leading-6 font-semibold text-text"
      id="page-title"
      tabindex="-1"
    >
      {#if searching}No {name} match "{library.query.trim()}"{:else if filter === "deleted"}Nothing
        was deleted{:else}No {name} yet{/if}
    </h2>
    <p class="max-w-95">
      {#if searching}
        Search looks at titles, tags, domains and usernames. It never looks inside secret values.
        Try fewer letters.
      {:else if filter === "deleted"}
        Items you delete wait here until you restore them.
      {:else}
        Add one and it will show up here.
      {/if}
    </p>
    {#if !searching && filter !== "deleted"}
      <Button onclick={add}><Icon name="plus" />New item</Button>
    {/if}
  </div>
{:else}
  <p
    id="page-title"
    tabindex="-1"
    class="mx-2 text-(length:--text-xs) leading-4 text-text-tertiary"
  >
    {#if searching}
      {plural(rows.length, "item", "items")} for "{library.query.trim()}". Search matches titles,
      tags, domains and usernames, and never shows secret values.
    {:else}
      {plural(rows.length, "item", "items")}
    {/if}
  </p>
  <ul
    class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
  >
    {#each rows as item (item.id)}
      {@const info = kindInfo(item.kind)}
      <li>
        <button
          type="button"
          onclick={() => open(item)}
          aria-label="{item.title}, {item.subtitle || info.label}"
          class="grid min-h-14 w-full grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 py-2 pr-3 pl-4 text-left hover:bg-surface-sunken focus-visible:outline-offset-[-2px]"
        >
          <span
            class="grid size-8 place-items-center rounded-(--radius-sm) bg-surface-sunken text-text-secondary"
          >
            <Icon name={info.icon} size={18} />
          </span>
          <span class="grid min-w-0">
            <span class="truncate font-semibold">{item.title}</span>
            <span class="truncate text-(length:--text-sm) text-text-secondary">
              {item.subtitle || info.label}
            </span>
          </span>
          <Icon name="chevron-right" class="text-text-tertiary" />
        </button>
      </li>
    {/each}
  </ul>
{/if}
