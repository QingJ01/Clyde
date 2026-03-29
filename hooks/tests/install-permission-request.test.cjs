const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

test("registerHooks writes PermissionRequest as nested matcher/hooks entry", () => {
  const { registerHooks } = require("../install.js");

  const tmp = path.join(os.tmpdir(), `clyde-hooks-${process.pid}-${Date.now()}.json`);
  fs.writeFileSync(tmp, JSON.stringify({
    hooks: {
      PermissionRequest: [
        { type: "http", url: "http://127.0.0.1:23333/permission", timeout: 600 },
      ],
    },
  }, null, 2));

  try {
    registerHooks({
      silent: true,
      settingsPath: tmp,
      claudeVersionInfo: { version: "9.9.9", source: "test", status: "known" },
    });

    const parsed = JSON.parse(fs.readFileSync(tmp, "utf8"));
    const entries = parsed.hooks.PermissionRequest;
    assert.equal(entries.length, 1);
    assert.equal(entries[0].matcher, "");
    assert.equal(entries[0].hooks.length, 1);
    assert.equal(entries[0].hooks[0].type, "http");
    assert.match(entries[0].hooks[0].url, /\/permission$/);
    assert.equal(entries[0].url, undefined);
  } finally {
    try { fs.unlinkSync(tmp); } catch {}
  }
});
