import { invoke } from "@tauri-apps/api/core";

/** What the window may know about the vault. Keys and plaintext stay in the Rust side (ADR-0020). */
export type VaultState = { initialised: boolean; version: string };

export const vaultState = () => invoke<VaultState>("vault_state");
