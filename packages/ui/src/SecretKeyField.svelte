<script lang="ts">
  /** Eight groups of four after the fixed `FL1-` prefix, as printed on the Emergency Kit. */
  type Props = {
    /** The whole key as typed so far, `FL1-XXXX-...`; groups that are still empty are left off. */
    value: string;
    error?: string;
    help?: string;
    disabled?: boolean;
    label?: string;
    focus?: boolean;
  };

  let {
    value = $bindable(),
    error,
    help,
    disabled = false,
    label = "Secret Key",
    focus = false,
  }: Props = $props();

  const GROUPS = 8;
  const SIZE = 4;
  const id = $props.id();
  let boxes = $state<HTMLInputElement[]>([]);
  let groups = $state<string[]>(Array(GROUPS).fill(""));

  const clean = (s: string) => s.toUpperCase().replace(/[^A-Z0-9]/g, "");

  // Follow a value set from outside (cleared after a failed attempt, or a test).
  $effect(() => {
    const body = value.startsWith("FL1") ? clean(value.slice(3)) : clean(value);
    const next = Array.from({ length: GROUPS }, (_, i) => body.slice(i * SIZE, (i + 1) * SIZE));
    if (next.join("-") !== groups.join("-")) groups = next;
  });

  $effect(() => {
    if (focus) boxes[0]?.focus();
  });

  function publish() {
    const typed = groups.map(clean);
    value = typed.every((g) => g === "") ? "" : `FL1-${typed.join("-")}`;
  }

  /** Spread typed or pasted text over this group and the ones after it. */
  function fill(from: number, text: string) {
    let chars = clean(text);
    // The printed key starts with FL1, which is not part of any group (L never appears inside one). Typed one
    // character at a time it arrives in the first box; pasted, it can land in any box.
    if (chars.startsWith("FL1") && (from === 0 || chars.length > SIZE)) {
      from = 0;
      chars = chars.slice(3);
    }
    const next = [...groups];
    // Emptying a box, or typing only the prefix into it, clears it.
    if (chars === "") next[from] = "";
    let at = from;
    while (chars.length > 0 && at < GROUPS) {
      next[at] = chars.slice(0, SIZE);
      chars = chars.slice(SIZE);
      at += 1;
    }
    groups = next;
    publish();
    // On to the first group still short of four characters, or the last one.
    const target = next.findIndex((g, j) => j >= from && g.length < SIZE);
    boxes[target === -1 ? GROUPS - 1 : target]?.focus();
  }

  function oninput(i: number, e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    fill(i, el.value);
    el.value = groups[i];
  }

  function onpaste(i: number, e: ClipboardEvent) {
    e.preventDefault();
    fill(i, e.clipboardData?.getData("text") ?? "");
  }

  function onkeydown(i: number, e: KeyboardEvent) {
    if (e.key === "Backspace" && groups[i] === "" && i > 0) {
      e.preventDefault();
      boxes[i - 1]?.focus();
    }
  }
</script>

<fieldset class="m-0 min-w-0 border-0 p-0" aria-describedby={error ? `${id}-err` : `${id}-help`}>
  <legend class="mb-1 p-0 text-(length:--text-sm) leading-5 font-medium">{label}</legend>
  <div class="flex flex-wrap items-center gap-2">
    <span
      class="flex h-(--control-height-md) items-center rounded-(--radius-sm) bg-surface-sunken px-3 font-mono text-(length:--text-sm) text-text-secondary"
      aria-hidden="true">FL1</span
    >
    {#each groups as group, i (i)}
      <input
        bind:this={boxes[i]}
        value={group}
        {disabled}
        oninput={(e) => oninput(i, e)}
        onpaste={(e) => onpaste(i, e)}
        onkeydown={(e) => onkeydown(i, e)}
        name="secret-key-{i + 1}"
        aria-label="{label} group {i + 1} of {GROUPS}"
        autocomplete="off"
        spellcheck="false"
        aria-invalid={error ? "true" : undefined}
        class="h-(--control-height-md) w-[calc(4ch+1.75rem)] rounded-(--radius-sm) border bg-surface px-1 text-center font-mono text-text uppercase disabled:opacity-50 {error
          ? 'border-danger'
          : 'border-border-strong'}"
      />
    {/each}
  </div>
  {#if error}
    <p id="{id}-err" class="mt-1 text-(length:--text-xs) leading-4 text-danger">{error}</p>
  {:else if help}
    <p id="{id}-help" class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">{help}</p>
  {/if}
</fieldset>
