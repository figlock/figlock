import type { Kind } from "./items";

/** What the sidebar lists. */
export type Filter = "all" | Kind | "deleted";

export type Route =
  | { p: "list"; f: Filter }
  | { p: "item"; id: string; from: Filter }
  | { p: "pick"; from: Filter }
  | { p: "generator" }
  | { p: "health" }
  | { p: "settings" }
  | { p: "edit"; id?: string; kind?: Kind; from: Filter };

/** The page history behind the back and forward buttons, and a guard for unsaved edits. */
export class Nav {
  stack = $state<Route[]>([{ p: "list", f: "all" }]);
  index = $state(0);
  /** An action waiting on the person's answer to "discard your changes?". */
  pending = $state<(() => void) | null>(null);
  /** Set by the editor while it has unsaved changes. */
  dirty: () => boolean = () => false;
  /** Scroll offset of each page, to come back to it. */
  scroll = new Map<number, number>();

  current = $derived(this.stack[this.index]);
  canBack = $derived(this.index > 0);
  canForward = $derived(this.index < this.stack.length - 1);

  /** Run `action` now, or after the person agrees to drop unsaved changes. */
  attempt(action: () => void) {
    if (this.current.p === "edit" && this.dirty()) this.pending = action;
    else action();
  }

  go(route: Route) {
    this.attempt(() => this.push(route));
  }

  /** Move to a page without the guard, for after a save. */
  push(route: Route) {
    this.stack = [...this.stack.slice(0, this.index + 1), route];
    this.index = this.stack.length - 1;
    this.#forget();
  }

  /** Scroll offsets of pages that are no longer in the history. */
  #forget() {
    for (const k of this.scroll.keys()) if (k >= this.stack.length - 1) this.scroll.delete(k);
  }

  /** Swap the current page for another, so back does not return to it. */
  replace(route: Route) {
    this.stack = this.stack.map((r, i) => (i === this.index ? route : r));
  }

  back() {
    if (this.canBack) this.attempt(() => this.index--);
  }

  forward() {
    if (this.canForward) this.attempt(() => this.index++);
  }

  /** After a save or delete: close the editor or item page, and the type picker that led to it, then
   * show `route` if given, else the page before. */
  leave(route?: Route) {
    let end = this.index;
    if (this.stack[end - 1]?.p === "pick") end--;
    const kept = this.stack.slice(0, Math.max(end, 1));
    this.stack = route ? [...kept, route] : kept;
    this.index = this.stack.length - 1;
    this.#forget();
  }

  confirmPending() {
    const action = this.pending;
    this.pending = null;
    this.dirty = () => false;
    action?.();
  }

  cancelPending() {
    this.pending = null;
  }
}
