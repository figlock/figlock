<script lang="ts">
  import { Button, Icon, SelectField, TextArea, TextField } from "@figlock/ui";
  import { onDestroy, onMount, tick } from "svelte";

  import {
    blankDraft,
    draftFromItem,
    hasErrors,
    keeps,
    newEntry,
    newPair,
    parseEnv,
    PROVIDERS,
    toItemDraft,
    validate,
    type Draft,
    type Errors,
  } from "./draft";
  import FilesEditor from "./FilesEditor.svelte";
  import {
    addItem,
    clearStaged,
    itemForEdit,
    kindInfo,
    updateItem,
    type FileInfo,
    type Kind,
  } from "./items";
  import ListEditor from "./ListEditor.svelte";
  import type { Filter } from "./nav.svelte";
  import PairsEditor from "./PairsEditor.svelte";
  import SecretInput from "./SecretInput.svelte";
  import TagsEditor from "./TagsEditor.svelte";
  import { toast } from "./toast.svelte";
  import { failure, getWorkspace } from "./workspace";

  type Props = { id?: string; kind?: Kind; from: Filter };
  let { id, kind, from }: Props = $props();

  const ws = getWorkspace();
  const { nav, library } = ws;

  let draft = $state<Draft | null>(null);
  let original = $state<FileInfo[]>([]);
  let snapshot = $state("");
  let errors = $state<Errors>({});
  let saveError = $state("");
  let saving = $state(false);
  let loadError = $state("");
  let pasting = $state(false);
  let pasted = $state("");
  let root = $state<HTMLElement>();

  const serial = (d: Draft) => JSON.stringify(d);
  const dirty = $derived(draft !== null && serial(draft) !== snapshot);

  onMount(async () => {
    if (id) {
      try {
        const item = await itemForEdit(id);
        original = item.files;
        draft = draftFromItem(item);
      } catch (e) {
        loadError = failure(ws, e);
        return;
      }
    } else {
      draft = blankDraft(kind ?? "login");
    }
    snapshot = serial(draft);
  });
  onDestroy(() => {
    nav.dirty = () => false;
    void clearStaged();
  });

  $effect(() => {
    nav.dirty = () => dirty;
  });

  const info = $derived(kindInfo(draft?.kind ?? kind ?? "login"));
  const providers = $derived.by(() => {
    const current = draft?.scalars.provider ?? "";
    const names = current && !PROVIDERS.includes(current) ? [...PROVIDERS, current] : PROVIDERS;
    return [{ value: "", label: "Other" }, ...names.map((p) => ({ value: p, label: p }))];
  });

  async function focusFirst(first: string) {
    await tick();
    const target =
      first === "pairs"
        ? root?.querySelector<HTMLElement>("[aria-invalid='true']")
        : root?.querySelector<HTMLElement>(
            `[data-field='${first}'] input, [data-field='${first}'] textarea, [data-field='${first}'] button`,
          );
    target?.focus();
  }

  async function save() {
    if (!draft || saving) return;
    saveError = "";
    errors = validate(draft);
    if (hasErrors(errors)) return void focusFirst(errors.first!);
    saving = true;
    try {
      const payload = toItemDraft(draft, original);
      let target = id;
      if (id) await updateItem(id, payload);
      else target = await addItem(draft.kind, payload);
      await library.refresh();
      toast.show(`Saved ${draft.title.trim()}`);
      nav.dirty = () => false;
      nav.leave(id ? undefined : { p: "item", id: target!, from });
    } catch (e) {
      saveError = failure(ws, e);
    } finally {
      saving = false;
    }
  }

  function toggleClear(field: string) {
    if (!draft) return;
    draft.cleared = draft.cleared.includes(field)
      ? draft.cleared.filter((f) => f !== field)
      : [...draft.cleared, field];
  }

  function addPasted() {
    if (!draft) return;
    const parsed = parseEnv(pasted);
    const named = new Set(draft.vars.filter((v) => v.name).map((v) => v.name));
    const fresh = parsed.filter((p) => !named.has(p.name)).map((p) => newPair(p.name, p.value));
    const blank = draft.vars.filter((v) => v.name || v.value);
    draft.vars = [...blank, ...fresh];
    pasted = "";
    pasting = false;
  }

  /** The private key from a key file: its text goes into the field, nothing else is read. */
  async function keyFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file || !draft) return;
    if (file.size > 32 * 1024) {
      errors = { ...errors, private_key: "That file is over 32 KB, too large for a private key." };
      return;
    }
    draft.scalars.private_key = await file.text();
  }
</script>

<div bind:this={root} class="flex flex-col gap-5">
  {#if loadError}
    <p role="alert" class="text-danger">{loadError}</p>
    <div><Button onclick={() => nav.back()}>Go back</Button></div>
  {:else if draft}
    <div class="flex flex-col gap-1">
      <h1
        id="page-title"
        tabindex="-1"
        class="text-(length:--text-2xl) leading-7 font-semibold wrap-anywhere"
      >
        {id ? `Edit ${draft.title || info.label}` : `New ${info.label.toLowerCase()}`}
      </h1>
      <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
        {id
          ? "Changes are encrypted on this device before they sync."
          : "FigLock encrypts the item on this device before it syncs."}
      </p>
    </div>

    <div class="flex items-center gap-3">
      <span
        class="grid size-8 place-items-center rounded-(--radius-sm) bg-surface-sunken text-text-secondary"
      >
        <Icon name={info.icon} size={18} />
      </span>
      <span class="grid">
        <span class="font-medium">{info.label}</span>
        <span class="text-(length:--text-xs) leading-4 text-text-tertiary">
          {id
            ? "The type cannot be changed once an item exists."
            : "To choose a different type, go back."}
        </span>
      </span>
    </div>

    <form
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
      class="flex flex-col gap-4 rounded-(--radius-md) border border-border bg-surface p-4"
    >
      <div data-field="title">
        <TextField
          label="Title"
          bind:value={draft.title}
          required
          maxlength={256}
          error={errors.title}
        />
      </div>

      {#if draft.kind === "login"}
        <TextField label="Username" bind:value={draft.scalars.username} maxlength={512} />
        <div data-field="password">
          <SecretInput
            label="Password"
            bind:value={draft.scalars.password}
            held={keeps(draft, "password")}
            required
            maxlength={4096}
            help="Up to 4 KB."
            error={errors.password}
          />
        </div>
        <div class="flex flex-col gap-3">
          <label class="flex items-center gap-4">
            <span class="grid min-w-0 flex-1">
              <span class="font-medium">Add one-time code</span>
              <span class="text-(length:--text-xs) leading-4 text-text-tertiary">
                Store the setup key from a site that offers two-step sign-in.
              </span>
            </span>
            <span class="relative inline-flex flex-none">
              <input
                type="checkbox"
                role="switch"
                bind:checked={draft.totpOn}
                class="peer absolute inset-0 z-10 m-0 size-full cursor-pointer opacity-0"
              />
              <span
                aria-hidden="true"
                class="h-5.5 w-9.5 rounded-full bg-border-strong transition-colors duration-(--duration-fast) peer-checked:bg-success peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-focus after:absolute after:top-0.5 after:left-0.5 after:size-4.5 after:rounded-full after:bg-[#fff] after:shadow after:transition-transform after:duration-(--duration-fast) after:content-[''] peer-checked:after:translate-x-4"
              ></span>
            </span>
          </label>
          {#if draft.totpOn}
            <div data-field="totp">
              <SecretInput
                label="One-time code setup key"
                bind:value={draft.scalars.totp}
                held={keeps(draft, "totp")}
                required
                maxlength={1024}
                error={errors.totp}
              />
            </div>
          {/if}
        </div>
        <ListEditor
          label="Websites"
          noun="Website"
          bind:entries={draft.urls}
          placeholder="https://example.com"
          maxlength={4096}
          onadd={() => (draft!.urls = [...draft!.urls, newEntry()])}
        />
      {:else if draft.kind === "token"}
        <SelectField label="Provider" bind:value={draft.scalars.provider} options={providers} />
        <div data-field="token">
          <SecretInput
            label="Token"
            bind:value={draft.scalars.token}
            held={keeps(draft, "token")}
            required
            maxlength={16384}
            help="Up to 16 KB."
            error={errors.token}
          />
        </div>
        <TextField
          label="Environment variable name"
          bind:value={draft.scalars.env_var}
          mono
          placeholder="GITHUB_TOKEN"
          maxlength={64}
          help="Used by figlock run."
        />
        <TextField label="Scopes" bind:value={draft.scalars.scopes} maxlength={1024} />
        <TextField
          label="Expires"
          type="date"
          bind:value={draft.scalars.expires_at}
          help="Optional. FigLock flags the item when it expires within 30 days."
        />
        <ListEditor
          label="Allowed hosts"
          noun="Host"
          bind:entries={draft.hosts}
          placeholder="api.example.com"
          mono
          maxlength={512}
          help="Agents can only use this token for these hosts."
          onadd={() => (draft!.hosts = [...draft!.hosts, newEntry()])}
        />
      {:else if draft.kind === "note"}
        <TextArea label="Note" bind:value={draft.scalars.body} rows={10} maxlength={262144} />
      {:else if draft.kind === "env"}
        <div data-field="vars">
          <PairsEditor
            label="Variables"
            noun="Variable"
            env
            bind:pairs={draft.vars}
            held={(p) => keeps(draft!, `env/${p.name}`)}
            error={errors.vars}
            help="Names use A-Z, 0-9 and underscore. Each value can be up to 64 KB."
            onadd={() => (draft!.vars = [...draft!.vars, newPair()])}
            onpaste={() => (pasting = !pasting)}
          />
        </div>
        {#if pasting}
          <div class="flex flex-col gap-2 rounded-(--radius-sm) bg-surface-sunken p-3">
            <TextArea label="Paste .env contents" bind:value={pasted} rows={6} mono />
            <div class="flex gap-2">
              <Button variant="primary" onclick={addPasted}>Add variables</Button>
              <Button onclick={() => (pasting = false)}>Cancel</Button>
            </div>
          </div>
        {/if}
      {:else if draft.kind === "ssh"}
        <div data-field="private_key" class="flex flex-col gap-2">
          <TextArea
            label="Private key"
            bind:value={draft.scalars.private_key}
            placeholder={keeps(draft, "private_key") ? "Unchanged" : undefined}
            rows={7}
            maxlength={32768}
            mono
            masked
            required
            help="Up to 32 KB."
            error={errors.private_key}
          />
          <div>
            <label class="inline-flex">
              <span
                class="inline-flex h-(--control-height-md) cursor-pointer items-center gap-2 rounded-(--radius-sm) border border-border-strong bg-surface px-4 text-(length:--text-sm) font-medium hover:bg-surface-sunken has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-focus"
              >
                <Icon name="upload" />Choose a key file
                <input type="file" class="sr-only" onchange={keyFile} />
              </span>
            </label>
          </div>
        </div>
        <TextArea
          label="Public key"
          bind:value={draft.scalars.public_key}
          rows={3}
          maxlength={8192}
          mono
          help="Optional."
        />
        <SecretInput
          label="Passphrase"
          bind:value={draft.scalars.passphrase}
          held={draft.held.includes("passphrase")}
          cleared={draft.cleared.includes("passphrase")}
          ontoggleclear={() => toggleClear("passphrase")}
          maxlength={4096}
          help="Optional."
        />
      {:else}
        <div data-field="files">
          <FilesEditor
            bind:kept={draft.kept}
            bind:staged={draft.staged}
            bind:error={errors.files}
          />
        </div>
        <SecretInput
          label="Password"
          bind:value={draft.scalars.password}
          held={draft.held.includes("password")}
          cleared={draft.cleared.includes("password")}
          ontoggleclear={() => toggleClear("password")}
          maxlength={4096}
          help="Optional. Up to 4 KB."
        />
        <ListEditor
          label="Websites"
          noun="Website"
          bind:entries={draft.urls}
          placeholder="https://example.com"
          maxlength={4096}
          onadd={() => (draft!.urls = [...draft!.urls, newEntry()])}
        />
        <TextField
          label="Expires"
          type="date"
          bind:value={draft.scalars.expires_at}
          help="Optional."
        />
      {/if}

      {#if draft.kind !== "note"}
        <TextArea label="Notes" bind:value={draft.scalars.notes} rows={3} maxlength={262144} />
      {/if}

      <TagsEditor bind:tags={draft.tags} />

      <PairsEditor
        label="Custom fields"
        noun="Custom field"
        bind:pairs={draft.custom}
        held={(p) => keeps(draft!, `custom/${p.id}/value`)}
        onadd={() => (draft!.custom = [...draft!.custom, newPair()])}
      />

      {#if saveError}
        <p role="alert" class="text-(length:--text-sm) text-danger">{saveError}</p>
      {/if}
      <div class="flex gap-2">
        <Button type="submit" variant="primary" disabled={saving}
          >{id ? "Save changes" : "Add item"}</Button
        >
        <Button onclick={() => nav.back()}>Cancel</Button>
      </div>
    </form>
  {:else}
    <p class="px-2 text-text-secondary">Loading...</p>
  {/if}
</div>
