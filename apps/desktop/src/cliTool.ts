import { invoke } from "@tauri-apps/api/core";

/**
 * `unavailable`: this build has no command line tool inside it. `moved`: our link points at another copy of the
 * app. `conflict`: something else is at the link's path and is left alone.
 * `external`: the command line tool is already installed on its own (Homebrew, the install script), so no second
 * copy is added.
 */
export type CliToolState =
  "unavailable" | "not_installed" | "installed" | "moved" | "conflict" | "external";

/** Paths are shown the way the person would type them, for example `~/.local/bin/figlock`. */
export type CliTool = {
  state: CliToolState;
  link: string;
  dir: string;
  /** Where the other install is, when `state` is `external`. */
  external: string | null;
};

export const cliToolStatus = () => invoke<CliTool>("cli_tool_status");
export const installCliTool = () => invoke<CliTool>("install_cli_tool");
export const removeCliTool = () => invoke<CliTool>("remove_cli_tool");
