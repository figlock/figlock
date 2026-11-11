/** How long a revealed secret stays on screen, and how long a copy stays on the clipboard (the core clears it). */
export const REVEAL_MS = 10_000;

export const fmtSize = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1_048_576
      ? `${Math.round(bytes / 1024)} KB`
      : `${(bytes / 1_048_576).toFixed(1)} MB`;

export const ageText = (ms: number, now = Date.now()) => {
  const days = Math.max(0, Math.floor((now - ms) / 86_400_000));
  if (days < 1) return "today";
  if (days === 1) return "yesterday";
  if (days < 60) return `${days} days ago`;
  if (days < 730) return `${Math.round(days / 30)} months ago`;
  return `${Math.round(days / 365)} years ago`;
};

const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());

/** Whole days from today to a `YYYY-MM-DD` date; negative when it is past. */
export const dayDiff = (date: string, now = new Date()) =>
  Math.round((new Date(`${date}T00:00:00`).getTime() - midnight(now).getTime()) / 86_400_000);

export const fmtDate = (date: string) =>
  new Date(`${date}T00:00:00`).toLocaleDateString([], {
    year: "numeric",
    month: "short",
    day: "numeric",
  });

export const expiryState = (date: string | undefined, now = new Date()) => {
  if (!date) return "";
  const d = dayDiff(date, now);
  return d < 0 ? "expired" : d <= 30 ? "soon" : "";
};

/** The `[id, value]` pairs of fields named `<prefix><id>`, in id order. */
export const suffixed = (fields: Record<string, string>, prefix: string) =>
  Object.keys(fields)
    .filter((f) => f.startsWith(prefix))
    .sort()
    .map((f) => [f.slice(prefix.length), fields[f]] as const);

/** A fresh field suffix: lowercase letters and digits, as the schema requires. */
export const newSuffix = () => {
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => "0123456789abcdefghijklmnopqrstuv"[b % 32]).join("");
};

/** Secret fields of an item type that the editor treats as a single masked value. */
export const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
