// Stands in for the Tauri app core in a browser, so Playwright can drive the real Svelte UI (FIG-91).
// It keeps a small in-memory vault and answers the same commands the Rust side does. Test data only.
// Options come from `window.__FAKE__` (set by e2e/core.ts before this runs); the state is on `window.__core`
// so a test can inspect it, for example to prove a secret never reached the window.
(() => {
  const opts = window.__FAKE__ ?? {};
  const PASSWORD = "correct horse battery";
  const SECRET_SCALARS = ["password", "totp", "token", "private_key", "passphrase"];
  const isSecret = (k) =>
    SECRET_SCALARS.includes(k) || k.startsWith("env/") || /^custom\/[^/]+\/value$/.test(k);

  const core = (window.__core = {
    phase: opts.phase ?? "unlocked",
    items: new Map(),
    deleted: new Set(),
    seq: 0,
    calls: [],
    copied: [],
    sync: {
      enabled: false,
      state: "off",
      signedIn: false,
      canSignIn: true,
      lastSyncSecsAgo: null,
      message: null,
      fork: false,
      needsUnlock: false,
      localFolder: "/Users/test/FigLock",
      ...(opts.sync ?? {}),
    },
    health: opts.health ?? null,
    failNext: null,
    cli: { state: "not_installed", ...(opts.cli ?? {}) },
    // What the update feed answers: `update` is `{ version, notes }` when there is a newer version.
    update: opts.update ?? null,
    updateState: opts.updateUnavailable ? "unavailable" : "idle",
    // The local vault folder is gone (FIG-118): `{ path, canRestore }`, or null when it is there.
    folderMissing: opts.folderMissing ?? null,
    // What the person picks in the folder dialog.
    pickedFolder: opts.pickedFolder ?? null,
  });

  const now = Date.now();
  const seed = (kind, fields, secrets, ageDays = 1) => {
    const id = `item-${++core.seq}`;
    core.items.set(id, {
      id,
      kind,
      fields,
      secrets,
      files: [],
      updatedMs: now - ageDays * 86_400_000,
    });
    return id;
  };
  for (const s of opts.items ?? []) seed(s.kind, s.fields, s.secrets ?? {}, s.ageDays);

  const state = () => ({
    phase: core.phase,
    version: "9.9.9",
    quickUnlock: false,
    touchAvailable: false,
    secretKeyKept: true,
    secretKeyProtection: "keychain",
    secretKeyHeld: true,
    autolockSecs: 300,
    recoveryEnabled: false,
    pendingKit: null,
    folderMissing: core.folderMissing,
  });

  const subtitle = (i) =>
    i.fields.username ?? i.fields.provider ?? (i.fields["url/a"] ? i.fields["url/a"] : "") ?? "";
  const summary = (i) => ({
    id: i.id,
    kind: i.kind,
    title: i.fields.title,
    subtitle: subtitle(i) || "",
    updatedMs: i.updatedMs,
  });
  const fail = (code, message) => Promise.reject({ code, message });
  const find = (id) =>
    core.items.get(id) ?? Promise.reject({ code: "not_found", message: "That item is gone." });

  // A tiny stand-in for the core's strength meter and generators. Not real randomness; the tests use the shape.
  const level = (bits) => (bits < 40 ? "weak" : bits < 75 ? "fair" : "strong");
  const bitsOf = (v) => {
    const pool =
      (/[a-z]/.test(v) ? 26 : 0) +
      (/[A-Z]/.test(v) ? 26 : 0) +
      (/[0-9]/.test(v) ? 10 : 0) +
      (/[^a-zA-Z0-9]/.test(v) ? 32 : 0);
    return Math.round(v.length * Math.log2(Math.max(pool, 2)));
  };
  const strength = (v) => ({ level: level(bitsOf(v)), bits: bitsOf(v) });
  const pick = (chars, n) =>
    Array.from({ length: n }, (_, i) => chars[(i * 7 + core.seq++) % chars.length]).join("");
  const WORDS = [
    "amber",
    "bridge",
    "cobalt",
    "delta",
    "ember",
    "falcon",
    "garnet",
    "harbor",
    "island",
    "juniper",
    "kettle",
    "lantern",
    "meadow",
    "nectar",
    "orchid",
    "pebble",
  ];

  const updateView = (state) => ({
    state,
    current: "9.9.9",
    version: state === "available" || state === "ready" ? core.update?.version : null,
    notes: state === "available" ? (core.update?.notes ?? null) : null,
  });

  const cliView = () => ({
    state: core.cli.state,
    link: "~/.local/bin/figlock",
    dir: "~/.local/bin",
    external: core.cli.state === "external" ? "/opt/homebrew/bin/figlock" : null,
  });

  const commands = {
    vault_state: () => state(),
    choose_folder: () => core.pickedFolder,
    locate_folder: ({ folder }) => {
      if (folder.includes("empty"))
        return fail("no_vault", "There is no FigLock vault in that folder.");
      core.folderMissing = null;
    },
    restore_from_drive: () => {
      core.folderMissing = null;
    },
    forget_vault: ({ confirmed }) => {
      if (!confirmed)
        return fail("not_confirmed", "Confirm that this Mac should stop using this vault.");
      core.folderMissing = null;
      core.phase = "none";
    },
    unlock: ({ password }) => {
      if (password !== PASSWORD) return fail("bad_password", "That master password is not right.");
      core.phase = "unlocked";
      return state();
    },
    lock: () => {
      core.phase = "locked";
    },
    activity: () => {},
    list_items: ({ query }) => {
      const q = (query ?? "").trim().toLowerCase();
      return [...core.items.values()]
        .filter((i) => !core.deleted.has(i.id))
        .filter((i) => !q || `${i.fields.title} ${subtitle(i)}`.toLowerCase().includes(q))
        .sort((a, b) => a.fields.title.localeCompare(b.fields.title))
        .map(summary);
    },
    list_deleted: () => [...core.items.values()].filter((i) => core.deleted.has(i.id)).map(summary),
    item_detail: ({ id }) => {
      const i = find(id);
      if (i instanceof Promise) return i;
      return {
        id,
        kind: i.kind,
        deleted: core.deleted.has(id),
        updatedMs: i.updatedMs,
        fields: i.fields,
        secrets: Object.keys(i.secrets),
        files: i.files,
      };
    },
    item_for_edit: ({ id }) => {
      const i = find(id);
      if (i instanceof Promise) return i;
      return {
        id,
        kind: i.kind,
        fields: i.fields,
        secrets: Object.keys(i.secrets),
        files: i.files,
      };
    },
    reveal_field: ({ id, field }) =>
      find(id).secrets?.[field] ?? fail("not_found", "No such field."),
    copy_field: ({ id, field }) => {
      core.copied.push(`${id}:${field}`);
      return 30;
    },
    copy_text: ({ value }) => {
      core.copied.push(`text:${value.length}`);
      return 30;
    },
    add_item: ({ kind, draft }) => {
      const fields = {};
      const secrets = {};
      for (const [k, v] of Object.entries(draft.fields)) (isSecret(k) ? secrets : fields)[k] = v;
      const id = `item-${++core.seq}`;
      core.items.set(id, { id, kind, fields, secrets, files: [], updatedMs: Date.now() });
      return id;
    },
    update_item: ({ id, draft }) => {
      const i = find(id);
      if (i instanceof Promise) return i;
      for (const k of draft.clear) delete i.secrets[k];
      const next = {};
      for (const [k, v] of Object.entries(draft.fields)) {
        if (isSecret(k)) i.secrets[k] = v;
        else next[k] = v;
      }
      i.fields = next;
      i.updatedMs = Date.now();
    },
    delete_item: ({ id }) => {
      core.deleted.add(id);
    },
    restore_item: ({ id }) => {
      core.deleted.delete(id);
    },
    clear_staged: () => {},
    generate_password: ({ spec }) => {
      const sets = [
        spec.lower && "abcdefghijkmnopqrstuvwxyz",
        spec.upper && "ABCDEFGHJKLMNPQRSTUVWXYZ",
        spec.digits && "23456789",
        spec.symbols && "!@#$%^&*",
      ].filter(Boolean);
      const value = pick(sets.join("") || "a", spec.length);
      return { value, strength: strength(value) };
    },
    generate_passphrase: ({ spec }) => {
      const sep = { hyphen: "-", dot: ".", space: " ", none: "" }[spec.separator];
      const words = Array.from({ length: spec.words }, (_, i) => {
        const w = WORDS[(i * 5 + core.seq++) % WORDS.length];
        return spec.capitalize ? w[0].toUpperCase() + w.slice(1) : w;
      });
      const value = words.join(sep) + (spec.addNumber ? `${sep}7` : "");
      return { value, strength: { level: "strong", bits: spec.words * 12 } };
    },
    password_strength: ({ value }) => strength(value),
    health_report: ({ checkBreaches }) => {
      if (core.health)
        return { ...core.health, breachChecked: checkBreaches && !core.health.breachFailed };
      const rows = [];
      for (const i of core.items.values()) {
        if (core.deleted.has(i.id) || !i.secrets.password) continue;
        const bits = bitsOf(i.secrets.password);
        if (bits < 40)
          rows.push({ id: i.id, weakBits: bits, reusedWith: 0, oldDays: null, breached: null });
      }
      return { checked: core.items.size, rows, breachChecked: checkBreaches, breachFailed: false };
    },
    sync_status: () => ({ ...core.sync }),
    update_check: () => {
      if (core.updateState === "unavailable") return updateView("unavailable");
      if (core.failUpdateCheck) return fail("update_check_failed", core.failUpdateCheck);
      if (!core.update) return updateView("up_to_date");
      core.updateState = "available";
      return updateView("available");
    },
    update_install: () => {
      core.updateState = "ready";
      return updateView("ready");
    },
    restart_app: () => {
      core.restarted = true;
    },
    cli_tool_status: () => cliView(),
    install_cli_tool: () => {
      core.cli.state = "installed";
      return cliView();
    },
    remove_cli_tool: () => {
      core.cli.state = "not_installed";
      return cliView();
    },
    sync_now: () => {
      core.sync = { ...core.sync, state: "idle", lastSyncSecsAgo: 0, message: null };
      return { ...core.sync };
    },
    sync_enable: () => {
      core.sync = {
        ...core.sync,
        enabled: true,
        signedIn: true,
        state: "idle",
        lastSyncSecsAgo: 0,
      };
      return { ...core.sync };
    },
    sync_disable: () => {
      core.sync = { ...core.sync, enabled: false, state: "off", lastSyncSecsAgo: null };
      return { ...core.sync };
    },
    drive_sign_in: () => {
      core.sync = { ...core.sync, signedIn: true };
      return { canSignIn: true, signedIn: true };
    },
    drive_status: () => ({ canSignIn: true, signedIn: core.sync.signedIn }),
    devices: () => [
      { id: "dev-1", name: "This Mac", fingerprint: "AB12-CD34", thisDevice: true, revoked: false },
    ],
    suggested_folder: () => "/Users/test/FigLock",
    "plugin:event|listen": () => 1,
    "plugin:event|unlisten": () => {},
  };

  let callbacks = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    transformCallback: () => ++callbacks,
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      core.calls.push(cmd);
      if (core.failNext && core.failNext.cmd === cmd) {
        const f = core.failNext;
        core.failNext = null;
        return fail(f.code, f.message);
      }
      const run = commands[cmd];
      if (!run) throw new Error(`the fake app core has no command ${cmd}`);
      return run(args ?? {});
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
