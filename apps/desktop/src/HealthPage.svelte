<script lang="ts">
  import { Alert, Badge, Button, Icon } from "@figlock/ui";
  import { onMount } from "svelte";

  import { healthReport, type HealthReport, type HealthRow } from "./health";
  import { kindInfo } from "./items";
  import { plural } from "./util";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  let report = $state<HealthReport | null>(null);
  let error = $state("");
  let checking = $state(false);

  async function run(checkBreaches: boolean) {
    checking = checkBreaches;
    error = "";
    try {
      report = await healthReport(checkBreaches);
    } catch (e) {
      error = failure(w, e);
    } finally {
      checking = false;
    }
  }

  // The local check needs no network, so it runs as the page opens.
  onMount(() => void run(false));

  const item = (id: string) => w.library.all.find((i) => i.id === id);
  const months = (days: number) => Math.floor(days / 30);
  const oldText = (days: number) =>
    days >= 730
      ? `Unchanged for ${Math.floor(days / 365)} years`
      : `Unchanged for ${months(days)} months`;

  const open = (row: HealthRow) => w.nav.go({ p: "item", id: row.id, from: "all" });
</script>

<div class="grid gap-4">
  <div>
    <h2 id="page-title" tabindex="-1" class="text-(length:--text-md) leading-6 font-semibold">
      Password health
    </h2>
    <p class="text-text-secondary">
      Finds weak, reused and old passwords on this Mac. Nothing leaves it unless you check for
      breaches.
    </p>
  </div>

  {#if error}<Alert>{error}</Alert>{/if}

  {#if report}
    {#if report.breachFailed}
      <Alert tone="warning">
        Could not reach the breach service, so breaches were not checked. The other results are
        below. Try again when you are online.
      </Alert>
    {/if}

    <section
      aria-labelledby="breach-title"
      class="grid gap-2 rounded-(--radius-md) border border-border bg-surface p-4"
    >
      <h3 id="breach-title" class="font-semibold">Known breaches</h3>
      <p class="text-text-secondary">
        FigLock sends only the first 5 characters of a scrambled (SHA-1) fingerprint of each
        password to Have I Been Pwned, which replies with every match for that start. The match is
        made here, so neither your passwords nor their full fingerprints are sent.
      </p>
      <div>
        <Button onclick={() => run(true)} disabled={checking}>
          <Icon name="shield-alert" />
          {checking ? "Checking..." : report.breachChecked ? "Check again" : "Check for breaches"}
        </Button>
      </div>
      {#if report.breachChecked}
        <p class="text-(length:--text-sm) text-text-secondary">
          Checked {plural(report.checked, "password", "passwords")} against known breaches.
        </p>
      {/if}
    </section>

    {#if report.rows.length === 0}
      <div class="flex flex-col items-center gap-3 px-6 py-10 text-center text-text-secondary">
        <div class="grid size-12 place-items-center rounded-full bg-success-subtle text-success">
          <Icon name="shield-check" size={24} />
        </div>
        <p class="font-semibold text-text">
          {#if report.checked === 0}No passwords to check yet{:else}No problems found{/if}
        </p>
        <p class="max-w-95">
          {#if report.checked === 0}
            Logins and documents with a password show up here.
          {:else}
            Checked {plural(report.checked, "password", "passwords")} for weak, reused and old ones{report.breachChecked
              ? ", and for known breaches"
              : ""}.
          {/if}
        </p>
      </div>
    {:else}
      <p class="mx-2 text-(length:--text-xs) leading-4 text-text-tertiary">
        {plural(report.rows.length, "item needs", "items need")} attention, out of {plural(
          report.checked,
          "password",
          "passwords",
        )}
      </p>
      <ul
        aria-label="Items that need attention"
        class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
      >
        {#each report.rows as row (row.id)}
          {@const it = item(row.id)}
          {@const info = kindInfo(it?.kind ?? "login")}
          <li>
            <button
              type="button"
              onclick={() => open(row)}
              aria-label="Open {it?.title ?? 'item'}"
              class="grid min-h-14 w-full grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 py-2 pr-3 pl-4 text-left hover:bg-surface-sunken focus-visible:outline-offset-[-2px]"
            >
              <span
                class="grid size-8 place-items-center rounded-(--radius-sm) bg-surface-sunken text-text-secondary"
              >
                <Icon name={info.icon} size={18} />
              </span>
              <span class="grid min-w-0 gap-1">
                <span class="truncate font-semibold">{it?.title ?? "Item"}</span>
                <span class="flex flex-wrap gap-1">
                  {#if row.breached !== null}
                    <Badge tone="danger">
                      In {plural(row.breached, "known breach", "known breaches")}
                    </Badge>
                  {/if}
                  {#if row.weakBits !== null}<Badge tone="danger">Weak</Badge>{/if}
                  {#if row.reusedWith > 0}
                    <Badge tone="warning">
                      Reused with {plural(row.reusedWith, "other item", "other items")}
                    </Badge>
                  {/if}
                  {#if row.oldDays !== null}<Badge tone="warning">{oldText(row.oldDays)}</Badge
                    >{/if}
                </span>
              </span>
              <Icon name="chevron-right" class="text-text-tertiary" />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if !error}
    <p class="text-text-secondary">Checking your passwords...</p>
  {/if}
</div>
