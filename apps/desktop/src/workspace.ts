import { getContext, setContext } from "svelte";

import type { Library } from "./library.svelte";
import type { Nav } from "./nav.svelte";
import { errorOf } from "./vault";

/** What every page of the unlocked app shares. */
export type Workspace = {
  nav: Nav;
  library: Library;
  /** A command said the vault is locked: go back to the unlock screen. */
  relock: () => void;
};

const KEY = Symbol("workspace");

export const setWorkspace = (w: Workspace) => setContext(KEY, w);
export const getWorkspace = () => getContext<Workspace>(KEY);

/** The message to show for a failed command; a locked vault sends the person to the unlock screen. */
export const failure = (w: Workspace, e: unknown) => {
  const err = errorOf(e);
  if (err.code === "locked") w.relock();
  return err.message;
};
