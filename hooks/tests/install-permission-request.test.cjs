const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { registerHooks, __test } = require("../install.js");
const { removeFlatHttpHooks } = __test;

function tmpSettings(content) {
  const tmp = path.join(os.tmpdir(), `clyde-hooks-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2)}.json`);
  fs.writeFileSync(tmp, JSON.stringify(content, null, 2));
  return tmp;
}

const REG_OPTS = {
  silent: true,
  claudeVersionInfo: { version: "9.9.9", source: "test", status: "known" },
};

// --- Task 1: flat → nested migration ---

test("flat PermissionRequest entry is rewritten to nested format", () => {
  const tmp = tmpSettings({
    hooks: {
      PermissionRequest: [
        { type: "http", url: "http://127.0.0.1:23333/permission", timeout: 600 },
      ],
    },
  });
  try {
    const result = registerHooks({ ...REG_OPTS, settingsPath: tmp });
    const parsed = JSON.parse(fs.readFileSync(tmp, "utf8"));
    const entries = parsed.hooks.PermissionRequest;
    assert.equal(entries.length, 1);
    assert.equal(entries[0].matcher, "");
    assert.equal(entries[0].hooks.length, 1);
    assert.equal(entries[0].hooks[0].type, "http");
    assert.match(entries[0].hooks[0].url, /\/permission$/);
    assert.equal(entries[0].url, undefined, "flat url field should not exist");
    assert.ok(result.migrated >= 1, "should report at least 1 migrated entry");

    const elicitationEntries = parsed.hooks.Elicitation;
    assert.equal(elicitationEntries.length, 1);
    assert.equal(elicitationEntries[0].matcher, "");
    assert.match(elicitationEntries[0].hooks[0].url, /\/elicitation$/);
  } finally {
    try { fs.unlinkSync(tmp); } catch {}
  }
});

test("already-nested PermissionRequest entry is preserved, not duplicated", () => {
  const tmp = tmpSettings({
    hooks: {
      PermissionRequest: [{
        matcher: "",
        hooks: [{ type: "http", url: "http://127.0.0.1:23333/permission", timeout: 600 }],
      }],
    },
  });
  try {
    registerHooks({ ...REG_OPTS, settingsPath: tmp });
    const parsed = JSON.parse(fs.readFileSync(tmp, "utf8"));
    const entries = parsed.hooks.PermissionRequest;
    assert.equal(entries.length, 1, "should not duplicate nested entry");
  } finally {
    try { fs.unlinkSync(tmp); } catch {}
  }
});

test("repeated registerHooks() is idempotent", () => {
  const tmp = tmpSettings({});
  try {
    registerHooks({ ...REG_OPTS, settingsPath: tmp });
    const first = JSON.parse(fs.readFileSync(tmp, "utf8"));
    registerHooks({ ...REG_OPTS, settingsPath: tmp });
    registerHooks({ ...REG_OPTS, settingsPath: tmp });
    const third = JSON.parse(fs.readFileSync(tmp, "utf8"));
    assert.equal(third.hooks.PermissionRequest.length, 1, "PermissionRequest should not accumulate");
    assert.equal(third.hooks.Elicitation.length, 1, "Elicitation should not accumulate");
    assert.deepStrictEqual(first.hooks.SessionStart, third.hooks.SessionStart, "SessionStart should not change across runs");
    assert.deepStrictEqual(first.hooks.PermissionRequest, third.hooks.PermissionRequest, "PermissionRequest should not change across runs");
    assert.deepStrictEqual(first.hooks.Elicitation, third.hooks.Elicitation, "Elicitation should not change across runs");
  } finally {
    try { fs.unlinkSync(tmp); } catch {}
  }
});

test("flat Elicitation entry is rewritten to nested format", () => {
  const tmp = tmpSettings({
    hooks: {
      Elicitation: [
        { type: "http", url: "http://127.0.0.1:23333/elicitation", timeout: 600 },
      ],
    },
  });
  try {
    const result = registerHooks({ ...REG_OPTS, settingsPath: tmp });
    const parsed = JSON.parse(fs.readFileSync(tmp, "utf8"));
    const entries = parsed.hooks.Elicitation;
    assert.equal(entries.length, 1);
    assert.equal(entries[0].matcher, "");
    assert.equal(entries[0].hooks.length, 1);
    assert.equal(entries[0].hooks[0].type, "http");
    assert.match(entries[0].hooks[0].url, /\/elicitation$/);
    assert.equal(entries[0].url, undefined, "flat url field should not exist");
    assert.ok(result.migrated >= 1, "should report at least 1 migrated entry");
  } finally {
    try { fs.unlinkSync(tmp); } catch {}
  }
});

test("mixed array: flat Clyde entry removed, unrelated hooks preserved", () => {
  const tmp = tmpSettings({
    hooks: {
      PermissionRequest: [
        { type: "http", url: "http://127.0.0.1:23333/permission", timeout: 600 },
        { type: "http", url: "https://example.com/my-permission-webhook", timeout: 30 },
        { matcher: "", hooks: [{ type: "command", command: "echo user-hook" }] },
      ],
    },
  });
  try {
    registerHooks({ ...REG_OPTS, settingsPath: tmp });
    const parsed = JSON.parse(fs.readFileSync(tmp, "utf8"));
    const entries = parsed.hooks.PermissionRequest;
    // Should have: the user's HTTP webhook, the user's command hook, and Clyde's nested entry
    const urls = entries.flatMap(e =>
      e.hooks ? e.hooks.map(h => h.url || h.command) : [e.url]
    ).filter(Boolean);
    assert.ok(urls.some(u => u.includes("example.com")), "unrelated HTTP hook preserved");
    assert.ok(urls.some(u => u === "echo user-hook"), "unrelated command hook preserved");
    assert.ok(urls.some(u => /127\.0\.0\.1.*\/permission$/.test(u)), "Clyde nested entry present");
  } finally {
    try { fs.unlinkSync(tmp); } catch {}
  }
});

// --- removeFlatHttpHooks unit tests ---

test("removeFlatHttpHooks: removes Clyde flat entry", () => {
  const entries = [
    { type: "http", url: "http://127.0.0.1:23333/permission", timeout: 600 },
  ];
  const result = removeFlatHttpHooks(entries, "/permission");
  assert.equal(result.entries.length, 0);
  assert.equal(result.removed, 1);
  assert.ok(result.changed);
});

test("removeFlatHttpHooks: preserves unrelated flat HTTP hook", () => {
  const entries = [
    { type: "http", url: "https://example.com/my-permission-webhook", timeout: 30 },
  ];
  const result = removeFlatHttpHooks(entries, "/permission");
  assert.equal(result.entries.length, 1, "unrelated hook should be preserved");
  assert.equal(result.removed, 0);
  assert.ok(!result.changed);
});

test("removeFlatHttpHooks: preserves nested format entries", () => {
  const entries = [
    { matcher: "", hooks: [{ type: "http", url: "http://127.0.0.1:23333/permission" }] },
  ];
  const result = removeFlatHttpHooks(entries, "/permission");
  assert.equal(result.entries.length, 1, "nested entry should not be removed");
  assert.equal(result.removed, 0);
});

test("removeFlatHttpHooks: handles different ports", () => {
  const entries = [
    { type: "http", url: "http://127.0.0.1:23335/permission", timeout: 600 },
    { type: "http", url: "http://127.0.0.1:23339/permission", timeout: 600 },
  ];
  const result = removeFlatHttpHooks(entries, "/permission");
  assert.equal(result.entries.length, 0, "all Clyde flat entries removed regardless of port");
  assert.equal(result.removed, 2);
});

test("removeFlatHttpHooks: does not match URL with permission as substring", () => {
  const entries = [
    { type: "http", url: "http://127.0.0.1:8080/api/permission-check" },
    { type: "http", url: "http://myserver.com/permission" },
  ];
  const result = removeFlatHttpHooks(entries, "/permission");
  assert.equal(result.entries.length, 2, "neither should match — wrong host or has path suffix");
  assert.equal(result.removed, 0);
});

test("removeFlatHttpHooks: uses endpoint-specific matching", () => {
  const entries = [
    { type: "http", url: "http://127.0.0.1:23333/permission", timeout: 600 },
    { type: "http", url: "http://127.0.0.1:23333/elicitation", timeout: 600 },
  ];
  const result = removeFlatHttpHooks(entries, "/elicitation");
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0].url, "http://127.0.0.1:23333/permission");
  assert.equal(result.removed, 1);
});
