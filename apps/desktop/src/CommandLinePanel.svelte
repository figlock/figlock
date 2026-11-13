<script lang="ts">
  import { Alert, Button, Icon } from "@figlock/ui";
  import { onMount } from "svelte";

  import { cliToolStatus, installCliTool, removeCliTool, type CliTool } from "./cliTool";
  import { errorOf } from "./vault";

  let info = $state<CliTool | null>(null);
  let busy = $state(false);
  let error = $state("");

  async function run(step: () => Promise<CliTool>) {
    error = "";
    busy = true;
    try {
      info = await step();
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }

  onMount(() => run(cliToolStatus));
</script>

<section aria-labelledby="cli-title" class="grid gap-3">
  <h3 id="cli-title" class="font-semibold">Command line tool</h3>

  {#if info === null}
    {#if !error}<p class="text-text-secondary">Checking...</p>{/if}
  {:else if info.state === "unavailable"}
    <p class="text-text-secondary" data-testid="cli-unavailable">
      This build of FigLock does not include the command line tool. Install a release build to get
      it.
    </p>
  {:else}
    <div class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
      <Icon
        name="terminal"
        class={info.state === "installed" ? "text-success" : "text-text-tertiary"}
      />
      <div class="min-w-0 flex-1">
        {#if info.state === "installed"}
          <p class="font-medium">Installed</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="cli-where">
            Terminal can run <span class="font-mono">figlock</span> from
            <span class="font-mono break-all">{info.link}</span>. It is always the same version as
            this app.
          </p>
        {:else if info.state === "moved"}
          <p class="font-medium">Needs repair</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="cli-where">
            <span class="font-mono break-all">{info.link}</span> points at a copy of FigLock that was
            moved or removed.
          </p>
        {:else if info.state === "external"}
          <p class="font-medium">Already installed</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="cli-external">
            The command line tool is already on this Mac at
            <span class="font-mono break-all">{info.external}</span>, so FigLock does not add
            another copy. Update it the way you installed it, for example with
            <span class="font-mono">brew upgrade figlock</span>.
          </p>
        {:else if info.state === "conflict"}
          <p class="font-medium">Not installed</p>
          <p class="text-(length:--text-sm) text-text-secondary">
            Lets you and your AI agents use FigLock from Terminal.
          </p>
        {:else}
          <p class="font-medium">Not installed</p>
          <p class="text-(length:--text-sm) text-text-secondary">
            Lets you and your AI agents use FigLock from Terminal. This adds one link at
            <span class="font-mono break-all">{info.link}</span>. It does not change your PATH or
            shell settings.
          </p>
        {/if}
      </div>
      {#if info.state === "not_installed"}
        <Button variant="primary" onclick={() => run(installCliTool)} disabled={busy}>
          {busy ? "Installing..." : "Install command line tool"}
        </Button>
      {:else if info.state === "moved"}
        <Button variant="primary" onclick={() => run(installCliTool)} disabled={busy}>
          Repair link
        </Button>
      {/if}
    </div>

    {#if info.state === "conflict"}
      <Alert tone="warning">
        Something else is already at <span class="font-mono break-all">{info.link}</span>, such as a
        Homebrew or script install, so FigLock left it alone. Remove it yourself if you want to use
        the tool from this app.
      </Alert>
    {/if}

    {#if info.state === "installed"}
      <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
        If Terminal says <span class="font-mono">figlock</span> is not found, open a new window. If
        it still is not, add
        <span class="font-mono break-all">{info.dir}</span> to your PATH, for example with
        <span class="font-mono break-all select-all">export PATH="$HOME/.local/bin:$PATH"</span> in your
        shell profile. FigLock never edits it for you.
      </p>
    {/if}

    {#if info.state === "installed" || info.state === "moved"}
      <div>
        <Button onclick={() => run(removeCliTool)} disabled={busy}>Remove the link</Button>
      </div>
    {/if}
  {/if}

  {#if error}<Alert>{error}</Alert>{/if}
</section>
