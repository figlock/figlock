<script lang="ts">
  import Button from "./Button.svelte";

  type Props = {
    title: string;
    body: string;
    confirm: string;
    cancel?: string;
    /** The confirm button is styled as destructive. */
    danger?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  };

  let {
    title,
    body,
    confirm,
    cancel = "Cancel",
    danger = false,
    onconfirm,
    oncancel,
  }: Props = $props();

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();

  $effect(() => {
    dialog?.showModal();
  });
</script>

<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  oncancel={(e) => {
    e.preventDefault();
    oncancel();
  }}
  class="m-auto w-[calc(100vw-2rem)] max-w-105 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl backdrop:bg-black/40"
>
  <div class="flex flex-col gap-3 p-6 pb-4">
    <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">{title}</h2>
    <p class="text-text-secondary">{body}</p>
  </div>
  <div class="grid grid-cols-2 gap-2 p-6 pt-2">
    <Button onclick={oncancel}>{cancel}</Button>
    <button
      type="button"
      onclick={onconfirm}
      class="inline-flex h-(--control-height-md) items-center justify-center rounded-(--radius-sm) px-4 text-(length:--text-sm) font-medium transition-colors duration-(--duration-fast) {danger
        ? 'bg-danger text-text-on-primary hover:brightness-90'
        : 'bg-primary text-text-on-primary hover:bg-primary-hover'}"
    >
      {confirm}
    </button>
  </div>
</dialog>
