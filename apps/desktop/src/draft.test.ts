import { expect, test } from "vitest";

import { blankDraft, draftFromItem, hasErrors, parseEnv, toItemDraft, validate } from "./draft";

const secret = () => crypto.randomUUID();

test("a login needs a title and a password, and a one-time code needs its key", () => {
  const d = blankDraft("login");
  expect(validate(d)).toMatchObject({
    title: expect.any(String),
    password: expect.any(String),
    first: "title",
  });
  d.title = "Site";
  d.scalars.password = secret();
  expect(hasErrors(validate(d))).toBe(false);
  d.totpOn = true;
  expect(validate(d).totp).toBeTruthy();
  d.scalars.totp = secret();
  expect(hasErrors(validate(d))).toBe(false);
});

test("each type asks for what its schema needs", () => {
  const token = blankDraft("token");
  token.title = "t";
  expect(validate(token).token).toBeTruthy();
  const ssh = blankDraft("ssh");
  ssh.title = "k";
  expect(validate(ssh).private_key).toBeTruthy();
  const doc = blankDraft("doc");
  doc.title = "d";
  expect(validate(doc).files).toBeTruthy();
  doc.staged = [{ token: "t1", name: "a.pdf", size: 3 }];
  expect(hasErrors(validate(doc))).toBe(false);
  const note = blankDraft("note");
  note.title = "n";
  expect(hasErrors(validate(note))).toBe(false);
});

test("env variables need unique upper-case names and a value", () => {
  const d = blankDraft("env");
  d.title = "Staging";
  expect(validate(d).vars).toBe("Add at least one variable.");
  d.vars = [
    { id: "a", name: "db_url", value: "x" },
    { id: "b", name: "KEY", value: "x" },
    { id: "c", name: "KEY", value: "y" },
    { id: "d", name: "EMPTY", value: "" },
    { id: "e", name: "", value: "orphan" },
  ];
  const e = validate(d);
  expect(e.first).toBe("pairs");
  expect(d.vars[0].error).toMatch(/start with a letter/);
  expect(d.vars[1].error).toBe("");
  expect(d.vars[2].error).toMatch(/already used/);
  expect(d.vars[3].error).toMatch(/Add a value/);
  expect(d.vars[4].error).toMatch(/Add a name/);
});

test("a custom field needs both its name and its value", () => {
  const d = blankDraft("note");
  d.title = "n";
  d.custom = [
    { id: "a", name: "Account", value: "" },
    { id: "b", name: "", value: secret() },
  ];
  expect(validate(d).first).toBe("pairs");
  expect(d.custom[0].error).toMatch(/Add a value/);
  expect(d.custom[1].error).toMatch(/Add a name/);
});

test("saving sends schema field ids, leaves empty values out and tags them with a marker value", () => {
  const d = blankDraft("login");
  const pw = secret();
  d.title = "  Cloudflare ";
  d.scalars.username = "ops@example.test";
  d.scalars.password = pw;
  d.scalars.totp = "ignored while the switch is off";
  d.urls = [
    { id: "u1", value: " dash.cloudflare.com " },
    { id: "u2", value: "" },
  ];
  d.tags = ["dns"];
  d.custom = [{ id: "c1", name: " Account ", value: "42" }];
  const out = toItemDraft(d, []);
  expect(out.fields).toEqual({
    title: "Cloudflare",
    username: "ops@example.test",
    password: pw,
    "url/u1": "dash.cloudflare.com",
    "tags/dns": "1",
    "custom/c1/name": "Account",
    "custom/c1/value": "42",
  });
  expect(out.addFiles).toEqual([]);
});

test("an item loaded for editing round-trips unchanged", () => {
  const pw = secret();
  const fields = {
    title: "GitHub token",
    provider: "GitHub",
    token: pw,
    "allowed_hosts/h1": "api.github.com",
    "tags/ci": "1",
    "custom/c1/name": "Org",
    "custom/c1/value": "acme",
  };
  const files = [{ id: "f1", name: "a.json", mime: "application/json", size: 2 }];
  const d = draftFromItem({ id: "x", kind: "token", fields, files });
  expect(d.hosts).toEqual([{ id: "h1", value: "api.github.com" }]);
  expect(d.tags).toEqual(["ci"]);
  expect(toItemDraft(d, files).fields).toEqual(fields);
  d.kept = [];
  expect(toItemDraft(d, files).removeFiles).toEqual(["f1"]);
});

test("a pasted .env file becomes names and values", () => {
  expect(
    parseEnv(
      [
        "# comment",
        "",
        "export DATABASE_URL=postgres://db.example.test/app",
        'QUOTED="a b"',
        "single='c'",
        "bad line",
        "lower_case=1",
      ].join("\n"),
    ),
  ).toEqual([
    { name: "DATABASE_URL", value: "postgres://db.example.test/app" },
    { name: "QUOTED", value: "a b" },
    { name: "SINGLE", value: "c" },
    { name: "LOWER_CASE", value: "1" },
  ]);
});
