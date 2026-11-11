import { invoke } from "@tauri-apps/api/core";

export type Level = "weak" | "fair" | "strong";
export type Strength = { level: Level; bits: number };
export type Generated = { value: string; strength: Strength };

export type Separator = "hyphen" | "dot" | "space" | "none";

export type PasswordSpec = {
  length: number;
  lower: boolean;
  upper: boolean;
  digits: boolean;
  symbols: boolean;
  avoidLookalikes: boolean;
};

export type PassphraseSpec = {
  words: number;
  separator: Separator;
  capitalize: boolean;
  addNumber: boolean;
};

/** Same bounds as the core generator, which refuses anything outside them. */
export const LENGTH = { min: 8, max: 128 };
export const WORDS = { min: 3, max: 16 };

export const DEFAULT_PASSWORD: PasswordSpec = {
  length: 20,
  lower: true,
  upper: true,
  digits: true,
  symbols: true,
  avoidLookalikes: false,
};

export const DEFAULT_PASSPHRASE: PassphraseSpec = {
  words: 6,
  separator: "hyphen",
  capitalize: false,
  addNumber: false,
};

export const SEPARATORS: { value: Separator; label: string }[] = [
  { value: "hyphen", label: "Hyphen" },
  { value: "dot", label: "Dot" },
  { value: "space", label: "Space" },
  { value: "none", label: "None" },
];

export const generatePassword = (spec: PasswordSpec) =>
  invoke<Generated>("generate_password", { spec });
export const generatePassphrase = (spec: PassphraseSpec) =>
  invoke<Generated>("generate_passphrase", { spec });
/** The same rating the generator's meter uses, for a password someone typed. */
export const passwordStrength = (value: string) => invoke<Strength>("password_strength", { value });
/** Copies in the app core with the timed clear. Resolves the seconds until it clears. */
export const copyText = (value: string) => invoke<number>("copy_text", { value });
