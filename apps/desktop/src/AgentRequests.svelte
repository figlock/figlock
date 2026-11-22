<script lang="ts">
  import { Alert, Badge, Button } from "@figlock/ui";
  import { onMount } from "svelte";

  import { agentDecide, agentRequests, onAgentDone, onAgentRequest, type AgentCard } from "./agent";

  /** Requests from programs on this Mac, one card at a time (ADR-0040). */
  let cards = $state<AgentCard[]>([]);
  /** The card whose system prompt is open. */
  let asking = $state<number | null>(null);
  let left = $state(0);
  let dialog = $state<HTMLDialogElement>();
  const titleId = $props.id();

  const card = $derived(cards[0]);

  function add(c: AgentCard) {
    if (!cards.some((x) => x.id === c.id)) cards = [...cards, c];
  }

  function remove(id: number) {
    cards = cards.filter((c) => c.id !== id);
    if (asking === id) asking = null;
  }

  onMount(() => {
    void agentRequests()
      .then((now) => now.forEach(add))
      .catch(() => {
        // Cards are an extra: with none, the system prompt appears on its own.
      });
    const offs = [onAgentRequest(add), onAgentDone(remove)].map((p) => p.catch(() => () => {}));
    const timer = setInterval(() => {
      if (left > 0) left -= 1;
    }, 1000);
    return () => {
      clearInterval(timer);
      offs.forEach((o) => void o.then((f) => f()));
    };
  });

  let shownId = $state<number | null>(null);
  $effect(() => {
    if (card && card.id !== shownId) {
      shownId = card.id;
      left = card.seconds;
    }
  });

  // Only mounted while there is a card, so the browser's modal state follows it.
  $effect(() => {
    dialog?.showModal();
  });

  async function decide(approve: boolean) {
    if (!card || asking !== null) return;
    const id = card.id;
    if (approve) asking = id;
    try {
      await agentDecide(id, approve);
    } finally {
      // The broker closes the card when it has its answer; this is for a card that is already gone.
      remove(id);
    }
  }
</script>

{#if card}
  <dialog
    bind:this={dialog}
    aria-labelledby={titleId}
    oncancel={(e) => {
      e.preventDefault();
      void decide(false);
    }}
    class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl"
    data-testid="agent-card"
  >
    <div class="flex flex-col gap-4 p-6">
      <div class="flex flex-col gap-1">
        <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
          {card.program} wants to use a secret
        </h2>
        <p class="text-text-secondary">
          A program on this Mac asked FigLock. Nothing is shared unless you approve it with Touch
          ID.
        </p>
      </div>

      {#each card.loud as warning (warning)}
        <Alert>{warning}</Alert>
      {/each}

      <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-2 text-(length:--text-sm)">
        <dt class="text-text-secondary">Wants</dt>
        <dd class="font-medium break-words">{card.wants}</dd>
        <dt class="text-text-secondary">For</dt>
        <dd>{card.duration}</dd>
        <dt class="text-text-secondary">How</dt>
        <dd>{card.how}</dd>
        <dt class="text-text-secondary">Program</dt>
        <dd class="break-all">
          {card.programPath}
          {#if card.programVerified}
            <Badge tone="success">checked by your Mac</Badge>
          {:else}
            <Badge tone="warning">could not be checked</Badge>
          {/if}
        </dd>
        {#if card.startedBy.length > 0}
          <dt class="text-text-secondary">Started by</dt>
          <dd>{card.startedBy.join(", then ")}</dd>
        {/if}
        {#if card.says}
          <dt class="text-text-secondary">It says</dt>
          <dd class="break-words">"{card.says}" <Badge>not checked</Badge></dd>
        {/if}
        {#if card.reason}
          <dt class="text-text-secondary">Reason</dt>
          <dd class="break-words">"{card.reason}" <Badge>not checked</Badge></dd>
        {/if}
      </dl>

      <div class="grid grid-cols-2 gap-2">
        <Button onclick={() => decide(false)} disabled={asking !== null}>Not now</Button>
        <Button variant="primary" onclick={() => decide(true)} disabled={asking !== null}>
          {asking === card.id ? "Waiting for Touch ID..." : "Approve with Touch ID"}
        </Button>
      </div>
      <p class="text-center text-(length:--text-xs) text-text-secondary" aria-live="off">
        No answer in {left} seconds counts as no.
      </p>
    </div>
  </dialog>
{/if}
