import { errorOf } from "./vault";
import { listDeleted, listItems, type ItemSummary } from "./items";

/** The items the sidebar and lists show. Rows hold no secret; search runs in the app core. */
export class Library {
  /** Every live item, for the counts and the lists. */
  all = $state<ItemSummary[]>([]);
  deleted = $state<ItemSummary[]>([]);
  query = $state("");
  /** The items matching `query`, best first; the same as `all` when it is empty. */
  hits = $state<ItemSummary[]>([]);
  loaded = $state(false);
  #timer: ReturnType<typeof setTimeout> | undefined;
  #seq = 0;

  constructor(private onlocked: () => void) {}

  /** Reload after a change. */
  async refresh() {
    try {
      const [all, deleted] = await Promise.all([listItems(""), listDeleted()]);
      this.all = all;
      this.deleted = deleted;
      await this.#search();
      this.loaded = true;
    } catch (e) {
      if (errorOf(e).code === "locked") this.onlocked();
    }
  }

  /** Search as the person types: the first keystroke runs at once, later ones settle for 40 ms. */
  search(query: string) {
    this.query = query;
    const idle = this.#timer === undefined;
    clearTimeout(this.#timer);
    if (idle || !query) {
      void this.#search();
      this.#timer = idle && query ? setTimeout(() => (this.#timer = undefined), 40) : undefined;
      return;
    }
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      void this.#search();
    }, 40);
  }

  async #search() {
    const seq = ++this.#seq;
    const query = this.query.trim();
    try {
      const hits = query ? await listItems(query) : this.all;
      // An older answer must not replace a newer one.
      if (seq === this.#seq) this.hits = hits;
    } catch (e) {
      if (errorOf(e).code === "locked") this.onlocked();
    }
  }

  stop() {
    clearTimeout(this.#timer);
    this.#timer = undefined;
  }
}
