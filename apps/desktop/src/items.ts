import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import type { IconName } from "@figlock/ui";

/** The item types of `figlock-core`'s schema. The ids are the ones the core stores. */
export type Kind = "login" | "token" | "note" | "env" | "ssh" | "doc";

export const KINDS: {
  kind: Kind;
  label: string;
  plural: string;
  icon: IconName;
  desc: string;
}[] = [
  {
    kind: "login",
    label: "Login",
    plural: "Logins",
    icon: "globe",
    desc: "A website sign-in with a password and an optional one-time code",
  },
  {
    kind: "token",
    label: "API token",
    plural: "API tokens",
    icon: "key-round",
    desc: "A token for an API, with the hosts agents may use it for",
  },
  {
    kind: "env",
    label: "Env bundle",
    plural: "Env bundles",
    icon: "braces",
    desc: "NAME and value pairs for a .env file, used by figlock run",
  },
  {
    kind: "ssh",
    label: "SSH key",
    plural: "SSH keys",
    icon: "key",
    desc: "A private key with its public key and passphrase",
  },
  {
    kind: "note",
    label: "Secure note",
    plural: "Notes",
    icon: "sticky-note",
    desc: "Private text, up to 256 KB",
  },
  {
    kind: "doc",
    label: "Document",
    plural: "Documents",
    icon: "file-text",
    desc: "Up to 8 files, encrypted on this device",
  },
];

export const kindInfo = (kind: string) => KINDS.find((k) => k.kind === kind) ?? KINDS[0];

/** The most files a document holds, and the most one file can be, in bytes (`figlock-core`). */
export const MAX_FILES = 8;
export const MAX_FILE_BYTES = 786_432;

/** One row of a list. Nothing secret. */
export type ItemSummary = {
  id: string;
  kind: Kind;
  title: string;
  subtitle: string;
  updatedMs: number;
};

export type FileInfo = { id: string; name: string; mime: string; size: number };

/** An item for its page. Secret values are not here: `secrets` only says which ones exist. */
export type ItemDetail = {
  id: string;
  kind: Kind;
  deleted: boolean;
  updatedMs: number;
  fields: Record<string, string>;
  secrets: string[];
  files: FileInfo[];
};

export type ItemForEdit = {
  id: string;
  kind: Kind;
  fields: Record<string, string>;
  files: FileInfo[];
};

/** What Save sends: every field the item should have, and the file changes. */
export type ItemDraft = {
  fields: Record<string, string>;
  addFiles: string[];
  removeFiles: string[];
};

export type StagedFile = { token: string; name: string; size: number };
export type Staged = { files: StagedFile[]; refused: { name: string; message: string }[] };

export const listItems = (query: string) => invoke<ItemSummary[]>("list_items", { query });
export const listDeleted = () => invoke<ItemSummary[]>("list_deleted");
export const itemDetail = (id: string) => invoke<ItemDetail>("item_detail", { id });
/** One secret value, because the person pressed Show. */
export const revealField = (id: string, field: string) =>
  invoke<string>("reveal_field", { id, field });
/** Copies in the app core, so the value never reaches the window. Resolves the seconds until it clears. */
export const copyField = (id: string, field: string) => invoke<number>("copy_field", { id, field });
export const addItem = (kind: Kind, draft: ItemDraft) =>
  invoke<string>("add_item", { kind, draft });
export const itemForEdit = (id: string) => invoke<ItemForEdit>("item_for_edit", { id });
export const updateItem = (id: string, draft: ItemDraft) =>
  invoke<void>("update_item", { id, draft });
export const deleteItem = (id: string) => invoke<void>("delete_item", { id });
export const restoreItem = (id: string) => invoke<void>("restore_item", { id });
export const chooseFiles = () => invoke<Staged>("choose_files");
export const stageFiles = (paths: string[]) => invoke<Staged>("stage_files", { paths });
export const unstageFile = (token: string) => invoke<void>("unstage_file", { token });
export const clearStaged = () => invoke<void>("clear_staged");
/** Resolves `false` when the person cancels the save dialog. */
export const saveFile = (id: string, fileId: string) =>
  invoke<boolean>("save_file", { id, fileId });

/** Files dragged over the window: whether they are over it, and their paths when dropped. The app core reads them. */
export const onFileDrag = (handler: (over: boolean, paths?: string[]) => void) =>
  getCurrentWebview().onDragDropEvent((e) => {
    const t = e.payload.type;
    if (t === "drop") handler(false, e.payload.paths);
    else handler(t === "enter" || t === "over");
  });
