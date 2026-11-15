import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  checkDocumentation,
  DOCUMENTATION_GUIDES,
} from "./check-doc-links.mjs";

function fixture(t) {
  const temporary = fs.realpathSync(os.tmpdir());
  const root = fs.mkdtempSync(path.join(temporary, "fono-doc-links-"));
  t.after(() => {
    // The recursive cleanup is limited to this newly created temporary child.
    assert.equal(path.dirname(path.resolve(root)), temporary);
    assert.match(path.basename(root), /^fono-doc-links-/);
    fs.rmSync(root, { recursive: true, force: true });
  });
  fs.mkdirSync(path.join(root, "docs"));
  for (const name of DOCUMENTATION_GUIDES) {
    fs.writeFileSync(path.join(root, "docs", name), `# ${name}\n`);
  }
  return {
    root,
    write(name, source) {
      const target = path.join(root, name);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, source);
    },
    check: () => checkDocumentation(root),
  };
}

test("twelve guides can link source resources and allow external URLs and anchors", (t) => {
  const f = fixture(t);
  f.write("assets/readme/preview.jpg", "fixture, not a distributed image");
  f.write(
    "docs/README.md",
    "[User](user-guide.md#settings) [Image](../assets/readme/preview.jpg)\n" +
      "[Upstream](https://example.invalid/not-requested) [Section](#local)\n",
  );
  f.write("README.md", "[Docs](docs/README.md)\n");
  assert.deepEqual(f.check().failures, []);
  assert.equal(f.check().checked, 3);
});

test("reports, binary artifacts, extra Markdown and directories cannot return to docs", (t) => {
  const f = fixture(t);
  for (const name of ["report.json", "installer.exe", "old-report.md"]) {
    f.write(`docs/${name}`, "unexpected");
  }
  fs.mkdirSync(path.join(f.root, "docs", "archive"));
  const failures = f.check().failures.join("\n");
  for (const name of [
    "report.json",
    "installer.exe",
    "old-report.md",
    "archive",
  ]) {
    assert.ok(failures.includes(`Unexpected documentation entry docs/${name}`));
  }
});

test("all twelve guides remain required", (t) => {
  const f = fixture(t);
  fs.unlinkSync(path.join(f.root, "docs", "api.md"));
  assert.ok(
    f.check().failures.includes("Missing documentation guide: docs/api.md"),
  );
});

test("missing local links fail even when a different link is external", (t) => {
  const f = fixture(t);
  f.write(
    "docs/server.md",
    "[Missing](missing.md) [External](https://example.invalid/missing.md)\n",
  );
  assert.deepEqual(f.check().failures, ["docs/server.md: missing missing.md"]);
});

test("license and resource README links are checked, not exempted as folders", (t) => {
  const f = fixture(t);
  f.write("licenses/README.md", "[Missing](missing-notice.txt)\n");
  f.write("licenses/other-NOTICE.md", "[Missing](missing-license.txt)\n");
  f.write("assets/readme/README.md", "[Missing](missing.jpg)\n");
  const failures = f.check().failures.join("\n");
  assert.match(failures, /licenses\/README.md: missing missing-notice.txt/);
  assert.match(
    failures,
    /licenses\/other-NOTICE.md: missing missing-license.txt/,
  );
  assert.match(failures, /assets\/readme\/README.md: missing missing.jpg/);
});

test("the two immutable notices cannot hide changed payloads behind their link exception", (t) => {
  const f = fixture(t);
  for (const name of [
    "offline-stt-NOTICE.md",
    "fono124-long-dictation-NOTICE.md",
  ]) {
    f.write(`licenses/${name}`, "changed payload [Legacy](../../../old.md)\n");
    assert.ok(
      f
        .check()
        .failures.includes(`Immutable notice bytes changed: licenses/${name}`),
    );
  }
});
