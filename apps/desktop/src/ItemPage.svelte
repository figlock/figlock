<script lang="ts">
  import { Badge, Button, ConfirmDialog, Icon } from "@figlock/ui";
  import { onMount } from "svelte";

  import {
    deleteItem,
    itemDetail,
    kindInfo,
    restoreItem,
    saveFile,
    type ItemDetail,
  } from "./items";
  import type { Filter } from "./nav.svelte";
  import PlainRow from "./PlainRow.svelte";
  import SecretRow from "./SecretRow.svelte";
  import { toast } from "./toast.svelte";
  import { ageText, dayDiff, expiryState, fmtDate, fmtSize, suffixed } from "./util";
  import { failure, getWorkspace } from "./workspace";

  type Props = { id: string; from: Filter };
  let { id, from }: Props = $props();

  const ws = getWorkspace();
  const { nav, library } = ws;

  let item = $state<ItemDetail | null>(null);
  let missing = $state(false);
  let error = $state("");
  let confirming = $state(false);

  async function load() {
    try {
      item = await itemDetail(id);
    } catch (e) {
      missing = true;
      error = failure(ws, e);
    }
  }
  onMount(load);

  const info = $derived(item ? kindInfo(item.kind) : null);
  const f = $derived(item?.fields ?? {});
  const title = $derived(f.title ?? "(untitled)");
  const urls = $derived(suffixed(f, "url/").map(([, v]) => v));
  const hosts = $derived(suffixed(f, "allowed_hosts/").map(([, v]) => v));
  const tags = $derived(
    Object.keys(f)
      .filter((k) => k.startsWith("tags/"))
      .map((k) => k.slice(5))
      .sort(),
  );
  const customs = $derived(
    Object.keys(f)
      .filter((k) => /^custom\/[^/]+\/name$/.test(k))
      .map((k) => ({ field: k.replace(/\/name$/, "/value"), name: f[k] })),
  );
  const envs = $derived((item?.secrets ?? []).filter((s) => s.startsWith("env/")).sort());
  const has = (secret: string) => item?.secrets.includes(secret) ?? false;
  const expiry = $derived(f.expires_at ? expiryState(f.expires_at) : "");

  async function remove() {
    confirming = false;
    try {
      await deleteItem(id);
      await library.refresh();
      toast.show(`Deleted ${title}`);
      nav.leave();
    } catch (e) {
      error = failure(ws, e);
    }
  }

  async function restore() {
    try {
      await restoreItem(id);
      await library.refresh();
      toast.show(`Restored ${title}`);
      await load();
    } catch (e) {
      error = failure(ws, e);
    }
  }

  async function download(fileId: string) {
    try {
      if (await saveFile(id, fileId)) toast.show("File saved");
    } catch (e) {
      error = failure(ws, e);
    }
  }

  const relative = (d: string) => {
    const n = dayDiff(d);
    const days = (c: number) => `${c} ${c === 1 ? "day" : "days"}`;
    return n < 0 ? `${days(-n)} ago` : n === 0 ? "Today" : `In ${days(n)}`;
  };
</script>

{#if missing}
  <div class="flex flex-col items-center gap-3 px-6 py-16 text-center">
    <h2 id="page-title" tabindex="-1" class="text-(length:--text-md) leading-6 font-semibold">
      This item is no longer in the vault
    </h2>
    {#if error}<p class="text-text-secondary">{error}</p>{/if}
    <Button onclick={() => nav.back()}>Go back</Button>
  </div>
{:else if item && info}
  <div class="flex items-center gap-4">
    <span
      class="grid size-14 flex-none place-items-center rounded-(--radius-md) bg-surface-sunken text-text-secondary"
    >
      <Icon name={info.icon} size={28} />
    </span>
    <div class="min-w-0 flex-1">
      <h1
        id="page-title"
        tabindex="-1"
        class="text-(length:--text-2xl) leading-7 font-semibold wrap-anywhere"
      >
        {title}
      </h1>
      <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
        {info.label}, updated {ageText(item.updatedMs)}
      </p>
    </div>
  </div>

  {#if item.deleted}
    <div
      class="flex items-center gap-3 rounded-(--radius-md) bg-warning-subtle px-4 py-3 text-text"
    >
      <Icon name="trash" size={20} class="text-warning" />
      <span class="min-w-0 flex-1">
        <span class="block font-semibold">This item is deleted</span>
        <span class="text-(length:--text-xs) leading-4 text-text-secondary">
          Restore it to see it in your vault again and to edit it.
        </span>
      </span>
      <Button onclick={restore}><Icon name="rotate-ccw" />Restore</Button>
    </div>
  {:else}
    <div class="flex flex-wrap gap-2">
      <Button onclick={() => nav.go({ p: "edit", id, from })}><Icon name="pencil" />Edit</Button>
      <button
        type="button"
        onclick={() => (confirming = true)}
        class="inline-flex h-(--control-height-md) items-center justify-center gap-2 rounded-(--radius-sm) px-4 text-(length:--text-sm) font-medium text-danger hover:bg-danger-subtle"
      >
        <Icon name="trash" />Delete
      </button>
    </div>
  {/if}

  {#if error}
    <p role="alert" class="text-(length:--text-sm) text-danger">{error}</p>
  {/if}

  {#if expiry}
    <div>
      <Badge tone={expiry === "expired" ? "danger" : "warning"}>
        <Icon name={expiry === "expired" ? "circle-alert" : "clock"} size={12} />
        {expiry === "expired" ? "Expired" : "Expires soon"}
      </Badge>
    </div>
  {/if}

  <div
    class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
  >
    {#if item.kind === "login"}
      <PlainRow label="Username" value={f.username} copy={{ id, field: "username" }} />
      {#if has("password")}<SecretRow {id} field="password" label="Password" />{/if}
      {#if has("totp")}<SecretRow {id} field="totp" label="One-time code setup key" />{/if}
      <PlainRow label="Websites" value={urls.join("\n")} />
      <PlainRow label="Notes" value={f.notes} />
    {:else if item.kind === "token"}
      {#if has("token")}<SecretRow {id} field="token" label="Token" />{/if}
      <PlainRow label="Provider" value={f.provider} />
      <PlainRow
        label="Environment variable name"
        value={f.env_var}
        mono
        copy={{ id, field: "env_var" }}
      />
      <PlainRow label="Scopes" value={f.scopes} />
      {#if f.expires_at}
        <PlainRow label="Expires">
          {fmtDate(f.expires_at)}
          <span class="block text-(length:--text-xs) leading-4 text-text-tertiary"
            >{relative(f.expires_at)}</span
          >
        </PlainRow>
      {/if}
      <PlainRow label="Allowed hosts" value={hosts.join("\n")} mono />
      <PlainRow label="Notes" value={f.notes} />
    {:else if item.kind === "note"}
      <PlainRow label="Note" value={f.body} />
    {:else if item.kind === "env"}
      {#each envs as field (field)}
        <SecretRow {id} {field} label={field.slice(4)} mono />
      {/each}
      <PlainRow label="Notes" value={f.notes} />
    {:else if item.kind === "ssh"}
      {#if has("private_key")}<SecretRow
          {id}
          field="private_key"
          label="Private key"
          mono
          multi
        />{/if}
      <PlainRow label="Public key" value={f.public_key} mono copy={{ id, field: "public_key" }} />
      {#if has("passphrase")}<SecretRow {id} field="passphrase" label="Passphrase" />{/if}
      <PlainRow label="Notes" value={f.notes} />
    {:else}
      {#if item.files.length}
        <div class="grid gap-1 px-4 py-3">
          <div class="text-(length:--text-sm) leading-5 font-medium text-text-secondary">Files</div>
          <ul class="divide-y divide-border">
            {#each item.files as file (file.id)}
              <li class="flex items-center gap-3 py-2">
                <Icon
                  name={/\.json$/i.test(file.name) ? "braces" : "file-text"}
                  size={20}
                  class="text-text-secondary"
                />
                <span class="grid min-w-0 flex-1">
                  <span class="truncate">{file.name}</span>
                  <span class="text-(length:--text-xs) leading-4 text-text-tertiary"
                    >{fmtSize(file.size)}</span
                  >
                </span>
                <Button onclick={() => download(file.id)} label="Download {file.name}">
                  <Icon name="download" />Download
                </Button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
      {#if has("password")}<SecretRow {id} field="password" label="Password" />{/if}
      <PlainRow label="Websites" value={urls.join("\n")} />
      {#if f.expires_at}
        <PlainRow label="Expires">
          {fmtDate(f.expires_at)}
          <span class="block text-(length:--text-xs) leading-4 text-text-tertiary"
            >{relative(f.expires_at)}</span
          >
        </PlainRow>
      {/if}
      <PlainRow label="Notes" value={f.notes} />
    {/if}
  </div>

  {#if customs.length}
    <section>
      <h2 class="mx-2 mb-2 text-(length:--text-sm) leading-5 font-medium text-text-secondary">
        Custom fields
      </h2>
      <div
        class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
      >
        {#each customs as c (c.field)}
          <SecretRow {id} field={c.field} label={c.name} />
        {/each}
      </div>
    </section>
  {/if}

  {#if tags.length}
    <section>
      <h2 class="mx-2 mb-2 text-(length:--text-sm) leading-5 font-medium text-text-secondary">
        Tags
      </h2>
      <div class="flex flex-wrap gap-2">
        {#each tags as tag (tag)}<Badge>{tag}</Badge>{/each}
      </div>
      <p class="mx-2 mt-2 text-(length:--text-xs) leading-4 text-text-tertiary">
        Search finds this item by its tags.
      </p>
    </section>
  {/if}

  {#if confirming}
    <ConfirmDialog
      title="Delete {title}?"
      body="It moves to Recently deleted, where you can restore it."
      confirm="Delete"
      danger
      onconfirm={remove}
      oncancel={() => (confirming = false)}
    />
  {/if}
{:else}
  <p class="px-2 text-text-secondary">Loading...</p>
{/if}
