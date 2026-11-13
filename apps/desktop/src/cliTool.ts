import { invoke } from "@tauri-apps/api/core";

/**
 * `unavailable`: this build has no command line tool inside it. `moved`: our link points at another copy of the
 * app. `conflict`: something else is at the link's path and is left alone.
 */
export type CliToolState = "unavailable" | "not_installed" | "installed" | "moved" | "conflict";

/** Paths are shown the way the person would type them, for example `~/.local/bin/figlock`. */
export type CliTool = { state: CliToolState; link: string; dir: string };

export const cliToolStatus = () => invoke<CliTool>("cli_tool_status");
export const installCliTool = () => invoke<CliTool>("install_cli_tool");
export const removeCliTool = () => invoke<CliTool>("remove_cli_tool");
