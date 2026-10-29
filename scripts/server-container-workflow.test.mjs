import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const workflow = (
  await readFile(
    new URL("../.github/workflows/server-container.yml", import.meta.url),
    "utf8",
  )
).replaceAll("\r\n", "\n");

function verify(source) {
  assert.doesNotMatch(source, /pull_request_target|self-hosted/);
  const build = source.split(/^  build:\n/m)[1]?.split(/^  publish:\n/m)[0];
  const publish = source.split(/^  publish:\n/m)[1];
  assert.ok(build && publish);
  assert.match(source, /^permissions:\n  contents: read\n/m);
  assert.doesNotMatch(build, /packages: write|secrets\.|docker (?:login|push)/);
  assert.match(build, /python3 scripts\/smoke-server-container\.py/);
  assert.match(
    publish,
    /^    if: github\.event_name == 'workflow_dispatch' && inputs\.publish$/m,
  );
  assert.match(publish, /^    needs: build$/m);
  assert.match(publish, /^    environment: fono-server-release$/m);
  for (const guard of [
    'test "$REF_TYPE" = tag',
    'test "$CONFIRM_SHA" = "$REVISION"',
    'test "$REF_NAME" = "v$version"',
  ]) {
    assert.ok(publish.includes(guard), guard);
    assert.ok(publish.indexOf(guard) < publish.indexOf("docker login"));
  }
  assert.match(
    publish,
    /gh run download "\$RUN_ID" --name fono-server-tested-image/,
  );
  assert.match(publish, /gzip -dc fono-server-tested\.tar\.gz \| docker load/);
}

test("container publication is manual, exact-revision and follows CPU smoke", () => {
  verify(workflow);
});

test("publication policy rejects removed gates and privileges in PR builds", () => {
  for (const mutate of [
    (s) =>
      s.replace(
        "  build:\n",
        "  build:\n    permissions:\n      packages: write\n",
      ),
    (s) =>
      s.replace(
        "github.event_name == 'workflow_dispatch' && inputs.publish\n    needs",
        "always()\n    needs",
      ),
    (s) => s.replace('test "$CONFIRM_SHA" = "$REVISION"', ": SHA gate removed"),
    (s) => s.replace('test "$REF_TYPE" = tag', ": tag gate removed"),
    (s) =>
      s.replace('test "$REF_NAME" = "v$version"', ": version gate removed"),
    (s) => s.replace("    needs: build", "    needs: []"),
    (s) =>
      s.replace("python3 scripts/smoke-server-container.py", "echo skip-smoke"),
  ]) {
    const changed = mutate(workflow);
    assert.notEqual(changed, workflow);
    assert.throws(() => verify(changed));
  }
});
