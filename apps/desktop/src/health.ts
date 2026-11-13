import { invoke } from "@tauri-apps/api/core";

/** One item with something wrong with its password. Ids and numbers only, never the password itself. */
export type HealthRow = {
  id: string;
  /** Guessability in bits, when the password is weak. */
  weakBits: number | null;
  /** How many other items use the same password. */
  reusedWith: number;
  /** Days since the item last changed, when that is a year or more. */
  oldDays: number | null;
  /** Times the password is in known breaches, when the breach check ran and found it. */
  breached: number | null;
};

export type HealthReport = {
  /** Passwords looked at. */
  checked: number;
  rows: HealthRow[];
  breachChecked: boolean;
  /** The breach check was asked for but the service could not be reached. */
  breachFailed: boolean;
};

/**
 * `checkBreaches` asks the Have I Been Pwned range service about each password. Only the first 5 hex
 * characters of its SHA-1 leave this Mac, never the password or its full hash.
 */
export const healthReport = (checkBreaches: boolean) =>
  invoke<HealthReport>("health_report", { checkBreaches });
