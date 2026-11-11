<script lang="ts">
  import { ConfirmDialog, Icon, Logo, Button, type IconName } from "@figlock/ui";
  import { onMount, tick } from "svelte";

  import GeneratorPage from "./GeneratorPage.svelte";
  import ItemEditor from "./ItemEditor.svelte";
  import ItemList from "./ItemList.svelte";
  import ItemPage from "./ItemPage.svelte";
  import { KINDS, kindInfo } from "./items";
  import { Library } from "./library.svelte";
  import { Nav, type Filter } from "./nav.svelte";
  import NewItem from "./NewItem.svelte";
  import { toast } from "./toast.svelte";
  import { setWorkspace } from "./workspace";

  type Props = {
    /** The person pressed Lock. */
    onlock: () => void;
    /** A command found the vault locked. */
    onlocked: (notice?: string) => void;
  };
  let { onlock, onlocked }: Props = $props();

  const nav = new Nav();
  const library = new Library(() => onlocked());
  setWorkspace({ nav, library, relock: (notice) => onlocked(notice) });

  let scroller = $state<HTMLElement>();
  let search = $state<HTMLInputElement>();
  let query = $state("");

  const route = $derived(nav.current);
  const section = $derived<Filter | null>(
    route.p === "list" ? route.f : route.p === "generator" ? null : route.from,
  );

  const NAMES: Record<string, string> = { all: "All items", deleted: "Recently deleted" };
  const nameOf = (f: Filter) => NAMES[f] ?? kindInfo(f).plural;
  const title = $derived(
    route.p === "generator"
      ? "Generator"
      : route.p === "pick" || (route.p === "edit" && !route.id)
        ? "New item"
        : route.p === "edit"
          ? "Edit item"
          : nameOf(section ?? "all"),
  );

  const count = (f: Filter) =>
    f === "all"
      ? library.all.length
      : f === "deleted"
        ? library.deleted.length
        : library.all.filter((i) => i.kind === f).length;

  const entries: { f: Filter; label: string; icon: IconName }[] = [
    { f: "all", label: "All items", icon: "layers" },
    ...KINDS.map((k) => ({ f: k.kind as Filter, label: k.plural, icon: k.icon })),
    { f: "deleted", label: "Recently deleted", icon: "trash" },
  ];

  onMount(() => {
    void library.refresh();
    return () => library.stop();
  });

  // Keep each page's scroll position for back and forward, and focus the new page's heading.
  let last = 0;
  $effect.pre(() => {
    const now = nav.index;
    if (now !== last && scroller) nav.scroll.set(last, scroller.scrollTop);
    return () => {
      last = now;
    };
  });
  $effect(() => {
    nav.current;
    const index = nav.index;
    void tick().then(() => {
      if (scroller) scroller.scrollTop = nav.scroll.get(index) ?? 0;
      document.getElementById("page-title")?.focus({ preventScroll: true });
    });
  });

  function select(f: Filter) {
    nav.go({ p: "list", f });
  }

  function onsearch() {
    library.search(query);
    if (query && route.p !== "list") nav.go({ p: "list", f: "all" });
  }

  const add = () =>
    section !== null && section !== "all" && section !== "deleted"
      ? nav.go({ p: "edit", kind: section, from: section })
      : nav.go({ p: "pick", from: section ?? "all" });

  function keys(e: KeyboardEvent) {
    if (!e.metaKey || e.altKey || e.ctrlKey) return;
    const act: Record<string, () => void> = {
      "[": () => nav.back(),
      "]": () => nav.forward(),
      f: () => search?.focus(),
      n: add,
      l: onlock,
    };
    const run = act[e.key];
    if (run) {
      e.preventDefault();
      run();
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div
  class="fixed inset-0 grid grid-cols-[248px_minmax(0,1fr)] bg-bg max-[860px]:grid-cols-[56px_minmax(0,1fr)]"
>
  <aside class="flex min-h-0 flex-col border-r border-border bg-surface-sunken">
    <div class="flex items-center gap-2 px-4 pt-4 pb-2 max-[860px]:justify-center max-[860px]:px-0">
      <Logo size={28} />
      <span class="font-semibold max-[860px]:hidden">FigLock</span>
    </div>
    <nav
      aria-label="Vault"
      class="flex min-h-0 flex-1 flex-col gap-px overflow-auto px-3 py-2 max-[860px]:px-2"
    >
      <h2
        class="px-2 pt-2 pb-1 text-(length:--text-xs) leading-4 font-medium text-text-tertiary max-[860px]:sr-only"
      >
        Vault
      </h2>
      {#each entries as e (e.f)}
        <button
          type="button"
          onclick={() => select(e.f)}
          aria-current={section === e.f && route.p !== "pick" ? "page" : undefined}
          aria-label={e.label}
          title={e.label}
          class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left font-medium hover:bg-border max-[860px]:justify-center max-[860px]:px-0 {section ===
            e.f && route.p !== 'pick'
            ? 'bg-border text-primary'
            : 'text-text'}"
        >
          <Icon name={e.icon} />
          <span class="min-w-0 flex-1 truncate max-[860px]:hidden">{e.label}</span>
          <span class="text-(length:--text-xs) text-text-tertiary tabular-nums max-[860px]:hidden">
            {count(e.f)}
          </span>
        </button>
      {/each}
      <h2
        class="px-2 pt-4 pb-1 text-(length:--text-xs) leading-4 font-medium text-text-tertiary max-[860px]:sr-only"
      >
        Tools
      </h2>
      <button
        type="button"
        onclick={() => nav.go({ p: "generator" })}
        aria-current={route.p === "generator" ? "page" : undefined}
        aria-label="Generator"
        title="Generator"
        class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left font-medium hover:bg-border max-[860px]:justify-center max-[860px]:px-0 {route.p ===
        'generator'
          ? 'bg-border text-primary'
          : 'text-text'}"
      >
        <Icon name="dices" />
        <span class="min-w-0 flex-1 truncate max-[860px]:hidden">Generator</span>
      </button>
    </nav>
    <div class="border-t border-border p-3 max-[860px]:p-2">
      <button
        type="button"
        onclick={onlock}
        title="Lock (Cmd+L)"
        aria-label="Lock"
        class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 font-medium text-text hover:bg-border max-[860px]:justify-center max-[860px]:px-0"
      >
        <Icon name="lock" /><span class="max-[860px]:hidden">Lock</span>
      </button>
    </div>
  </aside>

  <div class="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]">
    <header class="flex h-13 items-center gap-3 border-b border-border bg-surface px-4">
      <div class="inline-flex flex-none rounded-(--radius-sm) bg-surface-sunken">
        <button
          type="button"
          onclick={() => nav.back()}
          disabled={!nav.canBack}
          title="Back (Cmd+[)"
          class="grid h-7 w-8 place-items-center rounded-(--radius-sm) text-text hover:bg-border disabled:opacity-50 disabled:hover:bg-transparent"
        >
          <Icon name="chevron-left" /><span class="sr-only">Back</span>
        </button>
        <button
          type="button"
          onclick={() => nav.forward()}
          disabled={!nav.canForward}
          title="Forward (Cmd+])"
          class="grid h-7 w-8 place-items-center rounded-(--radius-sm) text-text hover:bg-border disabled:opacity-50 disabled:hover:bg-transparent"
        >
          <Icon name="chevron-right" /><span class="sr-only">Forward</span>
        </button>
      </div>
      <p class="min-w-0 flex-1 truncate text-(length:--text-md) leading-6 font-semibold">{title}</p>
      <div class="relative w-55 flex-none max-[720px]:w-35">
        <Icon
          name="search"
          class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-text-tertiary"
        />
        <input
          bind:this={search}
          bind:value={query}
          oninput={onsearch}
          type="search"
          placeholder="Search"
          aria-label="Search items"
          autocomplete="off"
          spellcheck="false"
          class="h-8 w-full rounded-(--radius-sm) border border-border-strong bg-surface pr-3 pl-9 text-text placeholder:text-text-tertiary"
        />
      </div>
      <Button variant="primary" onclick={add}>
        <Icon name="plus" /><span class="max-[720px]:sr-only">New item</span>
      </Button>
    </header>

    <div bind:this={scroller} class="min-h-0 overflow-auto bg-bg">
      <main class="mx-auto flex max-w-190 flex-col gap-5 px-8 pt-6 pb-12 max-[720px]:px-4">
        {#key `${nav.index}:${JSON.stringify(route)}`}
          {#if route.p === "list"}
            <ItemList filter={route.f} />
          {:else if route.p === "item"}
            <ItemPage id={route.id} from={route.from} />
          {:else if route.p === "generator"}
            <GeneratorPage />
          {:else if route.p === "pick"}
            <NewItem from={route.from} />
          {:else}
            <ItemEditor id={route.id} kind={route.kind} from={route.from} />
          {/if}
        {/key}
      </main>
    </div>
  </div>
</div>

<div
  role="status"
  aria-live="polite"
  class="pointer-events-none fixed bottom-6 left-1/2 z-(--z-toast) -translate-x-1/2"
>
  {#if toast.message}
    <p class="rounded-(--radius-sm) border border-border bg-surface-raised px-4 py-2 shadow-lg">
      {toast.message}
    </p>
  {/if}
</div>

{#if nav.pending}
  <ConfirmDialog
    title="Discard your changes?"
    body="You have edits that are not saved. FigLock will drop them if you leave this page."
    confirm="Discard changes"
    cancel="Keep editing"
    danger
    onconfirm={() => nav.confirmPending()}
    oncancel={() => nav.cancelPending()}
  />
{/if}
