import type { FileInfo, ItemDraft, ItemForEdit, Kind, StagedFile } from "./items";
import { newSuffix, suffixed } from "./util";

/** A row of a list field: a website or an allowed host, with the suffix it is stored under. */
export type Entry = { id: string; value: string };
/** A name and a secret value: an env variable or a custom field. */
export type Pair = { id: string; name: string; value: string; error?: string };

/** The item the editor is working on, in the shape the form needs. */
export type Draft = {
  kind: Kind;
  title: string;
  /** Plain one-per-field values by schema id (`username`, `password`, `notes`, ...). */
  scalars: Record<string, string>;
  totpOn: boolean;
  urls: Entry[];
  hosts: Entry[];
  vars: Pair[];
  custom: Pair[];
  tags: string[];
  /** Files the item already has and keeps; the ones taken out are sent as removals. */
  kept: FileInfo[];
  staged: StagedFile[];
};

/** The single-valued fields of each type, besides the title. */
export const SCALARS: Record<Kind, string[]> = {
  login: ["username", "password", "totp", "notes"],
  token: ["provider", "token", "env_var", "scopes", "expires_at", "notes"],
  note: ["body"],
  env: ["notes"],
  ssh: ["private_key", "public_key", "passphrase", "notes"],
  doc: ["password", "expires_at", "notes"],
};

/** Providers offered for a token. The empty one stores nothing. */
export const PROVIDERS = [
  "GitHub",
  "GitLab",
  "AWS",
  "Google Cloud",
  "Stripe",
  "OpenAI",
  "Anthropic",
  "Cloudflare",
];

export const ENV_NAME = /^[A-Z_][A-Z0-9_]*$/;
/** `env/NAME` must fit the 64-character field id. */
export const MAX_ENV_NAME = 60;

const entry = (value = ""): Entry => ({ id: newSuffix(), value });
const pair = (name = "", value = ""): Pair => ({ id: newSuffix(), name, value });

export const blankDraft = (kind: Kind): Draft => ({
  kind,
  title: "",
  scalars: SCALARS[kind].reduce<Record<string, string>>((o, f) => ({ ...o, [f]: "" }), {}),
  totpOn: false,
  urls: kind === "login" || kind === "doc" ? [entry()] : [],
  hosts: kind === "token" ? [entry()] : [],
  vars: kind === "env" ? [pair()] : [],
  custom: [],
  tags: [],
  kept: [],
  staged: [],
});

export function draftFromItem(item: ItemForEdit): Draft {
  const f = item.fields;
  const d = blankDraft(item.kind);
  d.title = f.title ?? "";
  for (const k of SCALARS[item.kind]) d.scalars[k] = f[k] ?? "";
  d.totpOn = !!f.totp;
  const urls = suffixed(f, "url/").map(([id, value]) => ({ id, value }));
  if (urls.length) d.urls = urls;
  const hosts = suffixed(f, "allowed_hosts/").map(([id, value]) => ({ id, value }));
  if (hosts.length) d.hosts = hosts;
  const vars = suffixed(f, "env/").map(([name, value]) => ({ id: newSuffix(), name, value }));
  if (vars.length) d.vars = vars;
  d.custom = Object.keys(f)
    .filter((k) => /^custom\/[^/]+\/name$/.test(k))
    .sort()
    .map((k) => {
      const id = k.split("/")[1];
      return { id, name: f[k], value: f[`custom/${id}/value`] ?? "" };
    });
  d.tags = Object.keys(f)
    .filter((k) => k.startsWith("tags/"))
    .map((k) => k.slice(5))
    .sort();
  d.kept = item.files;
  return d;
}

/** What Save sends. Empty values are left out, which removes them from the item. */
export function toItemDraft(d: Draft, original: FileInfo[]): ItemDraft {
  const fields: Record<string, string> = { title: d.title.trim() };
  for (const [k, v] of Object.entries(d.scalars)) {
    if (k === "totp" && !d.totpOn) continue;
    if (v.trim() !== "") fields[k] = v;
  }
  for (const e of d.urls) if (e.value.trim()) fields[`url/${e.id}`] = e.value.trim();
  for (const e of d.hosts) if (e.value.trim()) fields[`allowed_hosts/${e.id}`] = e.value.trim();
  for (const v of d.vars) if (v.name) fields[`env/${v.name}`] = v.value;
  for (const c of d.custom) {
    if (!c.name.trim()) continue;
    fields[`custom/${c.id}/name`] = c.name.trim();
    fields[`custom/${c.id}/value`] = c.value;
  }
  for (const t of d.tags) fields[`tags/${t}`] = "1";
  const keptIds = new Set(d.kept.map((k) => k.id));
  return {
    fields,
    addFiles: d.staged.map((s) => s.token),
    removeFiles: original.filter((o) => !keptIds.has(o.id)).map((o) => o.id),
  };
}

/** What is wrong with the draft, by the control it belongs to. Names the editor shows next to fields. */
export type Errors = {
  title?: string;
  password?: string;
  totp?: string;
  token?: string;
  private_key?: string;
  vars?: string;
  files?: string;
  /** The first control with a problem, to put focus on. */
  first?: string;
};

export function validate(d: Draft): Errors {
  const e: Errors = {};
  const s = d.scalars;
  if (!d.title.trim()) e.title = "Add a title so you can find this item later.";
  if (d.kind === "login") {
    if (!s.password) e.password = "Add a password. Login items need one.";
    if (d.totpOn && !s.totp.trim()) e.totp = "Paste the setup key, or turn off one-time code.";
  }
  if (d.kind === "token" && !s.token) e.token = "Paste the token you want to store.";
  if (d.kind === "ssh" && !s.private_key.trim())
    e.private_key = "Add the private key, or choose a key file.";
  if (d.kind === "env") {
    const seen = new Set<string>();
    for (const v of d.vars) {
      v.error = "";
      if (!v.name) {
        if (v.value) v.error = "Add a name for this variable.";
      } else if (!ENV_NAME.test(v.name)) {
        v.error = "Names start with a letter or underscore and use only A-Z, 0-9 and underscore.";
      } else if (seen.has(v.name)) {
        v.error = `${v.name} is already used. Each name must be unique.`;
      } else if (!v.value) {
        v.error = `Add a value for ${v.name}.`;
      }
      if (v.name) seen.add(v.name);
    }
    if (!d.vars.some((v) => v.name) && !d.vars.some((v) => v.error))
      e.vars = "Add at least one variable.";
  }
  if (d.kind === "doc" && d.kept.length + d.staged.length === 0) e.files = "Add at least one file.";
  for (const c of d.custom) {
    c.error = "";
    if (!c.name.trim() && c.value) c.error = "Add a name for this custom field.";
    else if (c.name.trim() && !c.value) c.error = `Add a value for ${c.name.trim()}.`;
  }
  const order = ["title", "password", "totp", "token", "private_key", "vars", "files"] as const;
  e.first =
    order.find((k) => e[k]) ?? (d.vars.concat(d.custom).some((p) => p.error) ? "pairs" : undefined);
  return e;
}

export const hasErrors = (e: Errors) => e.first !== undefined;

/** NAME=value lines of a pasted .env file. Comments, blank lines and `export` are skipped. */
export function parseEnv(text: string): { name: string; value: string }[] {
  const out: { name: string; value: string }[] = [];
  for (const raw of text.split(/\r?\n/)) {
    let line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    line = line.replace(/^export\s+/, "");
    const eq = line.indexOf("=");
    if (eq < 1) continue;
    const name = line
      .slice(0, eq)
      .trim()
      .toUpperCase()
      .replace(/[^A-Z0-9_]/g, "")
      .slice(0, MAX_ENV_NAME);
    let value = line.slice(eq + 1).trim();
    if (
      value.length >= 2 &&
      ((value.startsWith('"') && value.endsWith('"')) ||
        (value.startsWith("'") && value.endsWith("'")))
    ) {
      value = value.slice(1, -1);
    }
    if (name) out.push({ name, value });
  }
  return out;
}

export const newEntry = entry;
export const newPair = pair;
