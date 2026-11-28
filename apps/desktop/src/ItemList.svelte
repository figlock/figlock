<script lang="ts">
  import { Button, Icon } from "@figlock/ui";
  import { tick } from "svelte";

  import { kindInfo, type ItemSummary } from "./items";
  import type { Filter } from "./nav.svelte";
  import { plural } from "./util";
  import { getWorkspace } from "./workspace";

  type Props = {
    filter: Filter;
    /** Three-column layout: the list stays beside the open item. */
    split?: boolean;
    /** The open item, in the three-column layout. */
    selected?: string;
  };
  let { filter, split = false, selected }: Props = $props();

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
  // A big vault draws only the rows in view (fixed height, so the scroll bar stays true), which keeps the
  // list and search fast at 10,000 items. Small lists draw everything at their natural height.
  const WINDOW_FROM = 200;
  const ROW = 64;
  const OVERSCAN = 8;
  let box = $state<HTMLElement>();
  let top = $state(0);
  let viewport = $state(800);
  const windowed = $derived(rows.length > WINDOW_FROM);
  const first = $derived(windowed ? Math.max(0, Math.floor(top / ROW) - OVERSCAN) : 0);
  const last = $derived(
    windowed ? Math.min(rows.length, Math.ceil((top + viewport) / ROW) + OVERSCAN) : rows.length,
  );
  const shown = $derived(windowed ? rows.slice(first, last) : rows);

  const scroller = () => {
    for (let el = box?.parentElement; el; el = el.parentElement) {
      if (/(auto|scroll)/.test(getComputedStyle(el).overflowY)) return el;
    }
    return null;
  };
  function measure() {
    const s = scroller();
    if (!s || !box) return;
    top = Math.max(0, s.getBoundingClientRect().top - box.getBoundingClientRect().top);
    viewport = s.clientHeight;
  }
  $effect(() => {
    // Wait for the list to be drawn, then follow its scroll box.
    if (!box || !windowed) return;
    const s = scroller();
    if (!s) return;
    measure();
    s.addEventListener("scroll", measure, { passive: true });
    const resize = new ResizeObserver(measure);
    resize.observe(s);
    return () => {
      s.removeEventListener("scroll", measure);
      resize.disconnect();
    };
  });

  /** Bring a row into view, drawing it first when it is outside the window. */
  function reveal(index: number) {
    const s = scroller();
    if (!s || !box || !windowed) return;
    const y =
      index * ROW + box.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
    if (y < s.scrollTop) s.scrollTop = y;
    else if (y + ROW > s.scrollTop + s.clientHeight) s.scrollTop = y + ROW - s.clientHeight;
    measure();
  }

  const vaultEmpty = $derived(
    library.loaded && library.all.length === 0 && library.deleted.length === 0,
  );

  const open = (item: ItemSummary) =>
    split ? nav.select(filter, item.id) : nav.go({ p: "item", id: item.id, from: filter });

  /** Up and down move the selection through the list, Return on the open item edits it. */
  async function keys(e: KeyboardEvent, item: ItemSummary) {
    if (!split || e.metaKey || e.altKey || e.ctrlKey || e.shiftKey) return;
    if (e.key === "Enter" && item.id === selected && filter !== "deleted") {
      e.preventDefault();
      nav.go({ p: "edit", id: item.id, from: filter });
      return;
    }
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    const at = rows.findIndex((r) => r.id === item.id) + step;
    const next = rows[at];
    if (!next) return;
    e.preventDefault();
    open(next);
    reveal(at);
    await tick();
    document.getElementById(`row-${next.id}`)?.focus();
  }
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
    bind:this={box}
    style:padding-top={windowed ? `${first * ROW}px` : undefined}
    style:padding-bottom={windowed ? `${(rows.length - last) * ROW}px` : undefined}
    class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
  >
    {#each shown as item (item.id)}
      {@const info = kindInfo(item.kind)}
      <li style:height={windowed ? `${ROW}px` : undefined}>
        <button
          type="button"
          id="row-{item.id}"
          onclick={() => open(item)}
          onkeydown={(e) => keys(e, item)}
          aria-current={split && selected === item.id ? "true" : undefined}
          aria-label="{item.title}, {item.subtitle || info.label}"
          class="grid min-h-14 w-full {windowed
            ? 'h-full'
            : ''} grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 py-2 pr-3 pl-4 text-left hover:bg-surface-sunken focus-visible:outline-offset-[-2px] {split &&
          selected === item.id
            ? 'bg-primary-subtle hover:bg-primary-subtle'
            : ''}"
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
