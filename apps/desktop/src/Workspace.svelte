<script lang="ts">
  import { ConfirmDialog, Icon, Logo, Button, type IconName } from "@figlock/ui";
  import { onMount, tick } from "svelte";

  import GeneratorPage from "./GeneratorPage.svelte";
  import HealthPage from "./HealthPage.svelte";
  import AgentRequests from "./AgentRequests.svelte";
  import BrokerBanner from "./BrokerBanner.svelte";
  import RemovedBanner from "./RemovedBanner.svelte";
  import SecretKeyBanner from "./SecretKeyBanner.svelte";
  import Settings from "./Settings.svelte";
  import { onOpenAbout } from "./about";
  import { SyncStatus } from "./sync.svelte";
  import { badge } from "./sync";
  import ItemEditor from "./ItemEditor.svelte";
  import ItemList from "./ItemList.svelte";
  import ItemPage from "./ItemPage.svelte";
  import { KINDS, kindInfo } from "./items";
  import { Library } from "./library.svelte";
  import { Nav, type Filter } from "./nav.svelte";
  import NewItem from "./NewItem.svelte";
  import { toast } from "./toast.svelte";
  import { updates } from "./updates.svelte";
  import { setWorkspace } from "./workspace";

  type Props = {
    /** The person pressed Lock. */
    onlock: () => void;
    /** A command found the vault locked. */
    onlocked: (notice?: string) => void;
    /** When the vault removed this Mac (milliseconds), if it did: a banner says so and changes are refused. */
    removedAtMs?: number | null;
    /** The banner's button: back to the screen with the ways forward. */
    onremoved?: () => void;
  };
  let { onlock, onlocked, removedAtMs = null, onremoved = () => {} }: Props = $props();

  const nav = new Nav();
  const library = new Library(() => onlocked());
  // Items other devices added show up as the app core pulls them.
  const sync = new SyncStatus(() => void library.refresh());
  setWorkspace({ nav, library, sync, relock: (notice) => onlocked(notice) });
  const syncBadge = $derived(sync.view?.enabled ? badge(sync.view) : null);

  let scroller = $state<HTMLElement>();
  let search = $state<HTMLInputElement>();
  let query = $state("");
  /** Wide enough for the list and the open item side by side; narrower shows one page at a time. */
  let wide = $state(true);

  const route = $derived(nav.current);
  const split = $derived(wide && (route.p === "list" || route.p === "item"));
  /** The open item in the three-column layout. */
  const selected = $derived(
    route.p === "list" ? route.sel : route.p === "item" ? route.id : undefined,
  );
  const section = $derived<Filter | null>(
    route.p === "list"
      ? route.f
      : route.p === "generator" || route.p === "health" || route.p === "settings"
        ? null
        : route.from,
  );

  const SETTINGS_TITLES: Record<string, string> = {
    security: "Security",
    sync: "Sync",
    devices: "Devices",
    updates: "Updates",
    advanced: "Command line tool",
    export: "Export",
    import: "Import",
    about: "About FigLock",
  };
  const NAMES: Record<string, string> = { all: "All items", deleted: "Recently deleted" };
  const nameOf = (f: Filter) => NAMES[f] ?? kindInfo(f).plural;
  const title = $derived(
    route.p === "generator"
      ? "Generator"
      : route.p === "health"
        ? "Password health"
        : route.p === "settings"
          ? (SETTINGS_TITLES[route.page ?? ""] ?? "Settings")
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
    const query = window.matchMedia?.("(min-width: 1000px)");
    const onWidth = (e: MediaQueryListEvent) => {
      wide = e.matches;
      // Narrow again: the item that was open beside the list becomes the page on top of it.
      const r = nav.current;
      if (!wide && r.p === "list" && r.sel) nav.push({ p: "item", id: r.sel, from: r.f });
    };
    if (query) {
      wide = query.matches;
      query.addEventListener("change", onWidth);
    }
    void library.refresh();
    const stopSync = sync.start();
    // FigLock > About FigLock in the app menu.
    const stopAbout = onOpenAbout(() => nav.go({ p: "settings", page: "about" }));
    return () => {
      query?.removeEventListener("change", onWidth);
      library.stop();
      stopSync();
      void stopAbout.then((off) => off());
    };
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
  // Picking another item in the three-column layout keeps the list where it is and the focus on it.
  const pageKey = $derived(
    split ? `${nav.index}:split:${section}` : `${nav.index}:${JSON.stringify(route)}`,
  );
  $effect(() => {
    pageKey;
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

  const typing = (t: EventTarget | null) =>
    t instanceof HTMLElement &&
    (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));

  function keys(e: KeyboardEvent) {
    if (!e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey && !typing(e.target)) {
      if (e.key === "/") {
        e.preventDefault();
        search?.focus();
        return;
      }
    }
    // Esc leaves the editor, asking first when there are unsaved changes.
    if (
      e.key === "Escape" &&
      route.p === "edit" &&
      !nav.pending &&
      !document.querySelector('[role="dialog"],[role="alertdialog"]')
    ) {
      e.preventDefault();
      nav.back();
      return;
    }
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
          title={e.label}
          class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left font-medium max-[860px]:justify-center max-[860px]:px-0 {section ===
            e.f && route.p !== 'pick'
            ? 'bg-primary-subtle text-primary'
            : 'text-text hover:bg-border'}"
        >
          <Icon name={e.icon} />
          <span class="min-w-0 flex-1 truncate max-[860px]:sr-only">{e.label}</span>
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
        title="Generator"
        class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left font-medium max-[860px]:justify-center max-[860px]:px-0 {route.p ===
        'generator'
          ? 'bg-primary-subtle text-primary'
          : 'text-text hover:bg-border'}"
      >
        <Icon name="dices" />
        <span class="min-w-0 flex-1 truncate max-[860px]:sr-only">Generator</span>
      </button>
      <button
        type="button"
        onclick={() => nav.go({ p: "health" })}
        aria-current={route.p === "health" ? "page" : undefined}
        title="Password health"
        class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left font-medium max-[860px]:justify-center max-[860px]:px-0 {route.p ===
        'health'
          ? 'bg-primary-subtle text-primary'
          : 'text-text hover:bg-border'}"
      >
        <Icon name="shield-check" />
        <span class="min-w-0 flex-1 truncate max-[860px]:sr-only">Password health</span>
      </button>
      <button
        type="button"
        onclick={() => nav.go({ p: "settings" })}
        aria-current={route.p === "settings" ? "page" : undefined}
        title="Settings"
        class="flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left font-medium max-[860px]:justify-center max-[860px]:px-0 {route.p ===
        'settings'
          ? 'bg-primary-subtle text-primary'
          : 'text-text hover:bg-border'}"
      >
        <Icon name="settings" />
        <span class="min-w-0 flex-1 truncate max-[860px]:sr-only">Settings</span>
      </button>
    </nav>
    <div class="border-t border-border p-3 max-[860px]:p-2">
      {#if updates.pending}
        <button
          type="button"
          onclick={() => nav.go({ p: "settings", page: "updates" })}
          title="Updates"
          class="mb-1 flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left text-(length:--text-xs) font-medium text-text hover:bg-border max-[860px]:hidden"
        >
          <Icon name="download" class="text-primary" />
          <span class="min-w-0 flex-1 truncate" data-testid="update-badge"
            >{updates.view?.state === "ready" ? "Restart to update" : "Update available"}</span
          >
        </button>
      {/if}
      {#if syncBadge}
        <button
          type="button"
          onclick={() => nav.go({ p: "settings", page: "sync" })}
          title="Sync"
          class="mb-1 flex h-8 w-full items-center gap-2 rounded-(--radius-sm) px-2 text-left text-(length:--text-xs) hover:bg-border max-[860px]:hidden {syncBadge.attention
            ? 'text-warning'
            : 'text-text-secondary'}"
        >
          <Icon name={syncBadge.attention ? "triangle-alert" : "refresh"} />
          <span class="min-w-0 flex-1 truncate" data-testid="sync-badge">{syncBadge.text}</span>
        </button>
      {/if}
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

  <div class="grid min-h-0 min-w-0 grid-rows-[auto_auto_minmax(0,1fr)]">
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

    <div>
      {#if removedAtMs !== null}
        <RemovedBanner {removedAtMs} onoptions={onremoved} />
      {/if}
      <SecretKeyBanner />
      <BrokerBanner />
      <AgentRequests />
    </div>

    {#if split && section}
      <div class="grid min-h-0 grid-cols-[minmax(280px,360px)_minmax(0,1fr)] bg-bg">
        <div
          bind:this={scroller}
          class="min-h-0 overflow-auto border-r border-border px-3 pt-4 pb-8"
        >
          <section aria-label="Items" class="flex flex-col gap-3">
            {#key section}
              <ItemList filter={section} split {selected} />
            {/key}
          </section>
        </div>
        <div class="min-h-0 overflow-auto">
          <section
            aria-label="Item"
            class="mx-auto flex max-w-190 flex-col gap-5 px-8 pt-6 pb-12 max-[1100px]:px-5"
          >
            {#key selected}
              {#if selected}
                <ItemPage id={selected} from={section} split />
              {:else}
                <div class="flex flex-col items-center gap-2 px-6 py-24 text-center">
                  <p class="font-semibold text-text-secondary">No item selected</p>
                  <p class="text-(length:--text-sm) text-text-tertiary">
                    Pick an item from the list to see it here.
                  </p>
                </div>
              {/if}
            {/key}
          </section>
        </div>
      </div>
    {:else}
      <div bind:this={scroller} class="min-h-0 overflow-auto bg-bg">
        <main class="mx-auto flex max-w-190 flex-col gap-5 px-8 pt-6 pb-12 max-[720px]:px-4">
          {#key `${nav.index}:${JSON.stringify(route)}`}
            {#if route.p === "list"}
              <ItemList filter={route.f} />
            {:else if route.p === "item"}
              <ItemPage id={route.id} from={route.from} />
            {:else if route.p === "generator"}
              <GeneratorPage />
            {:else if route.p === "health"}
              <HealthPage />
            {:else if route.p === "settings"}
              <Settings />
            {:else if route.p === "pick"}
              <NewItem from={route.from} />
            {:else}
              <ItemEditor id={route.id} kind={route.kind} from={route.from} />
            {/if}
          {/key}
        </main>
      </div>
    {/if}
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
