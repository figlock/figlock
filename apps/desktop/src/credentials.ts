import { invoke } from "@tauri-apps/api/core";

/** `secretKey` is set only by a rotation: the new key, shown for the new Emergency Kit. */
export type Changed = { secretKey: string | null };

/**
 * Change the master password, rotate the Secret Key, or both. A blank `secretKey` means "use the one this Mac
 * keeps"; `needs_secret_key` says there is none to use.
 */
export const changeCredentials = (
  password: string,
  secretKey: string,
  newPassword: string | null,
  rotate: boolean,
) =>
  invoke<Changed>("change_credentials", {
    password,
    secretKey: secretKey || null,
    newPassword,
    rotate,
  });

/** Close the new-kit step; refused until the new key was confirmed with `checkKitKey`. */
export const endRotation = () => invoke<void>("end_rotation");
