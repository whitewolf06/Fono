import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = async (path) =>
  (await readFile(new URL(path, import.meta.url), "utf8")).replaceAll(
    "\r\n",
    "\n",
  );
const workflow = await read("../.github/workflows/ci.yml");
const tauriConfig = JSON.parse(await read("../src-tauri/tauri.conf.json"));
const releaseWorkflow = await read("../.github/workflows/prepare-update.yml");

// This checks the deliberately small job contract without an npm YAML parser:
// the test itself must run in an empty checkout before installing dependencies.
function nativeJob(source) {
  const jobs = [...source.matchAll(/^  ([a-z][\w-]*):\r?\n/gm)];
  const index = jobs.findIndex((match) => match[1] === "native");
  assert.notEqual(index, -1, "native job must exist");
  return source
    .slice(jobs[index].index, jobs[index + 1]?.index)
    .replace(/^\s*#.*$/gm, "");
}

function checkNativeContract(source) {
  const job = nativeJob(source);
  assert.match(source, /^  pull_request:\r?\n    branches: \[main\]/m);
  assert.doesNotMatch(source, /^  pull_request_target:/m);
  assert.doesNotMatch(
    source.split(/^jobs:/m)[0],
    /secrets\.|NODE_AUTH_TOKEN|WHITEUI_NPM_TOKEN/,
  );
  assert.match(job, /^    runs-on: windows-2022$/m);
  assert.doesNotMatch(job, /^\s+(?:needs|if):/m);
  assert.doesNotMatch(job, /^    (?:environment|uses):/m);
  assert.doesNotMatch(
    job,
    /secrets\.|NODE_AUTH_TOKEN|WHITEUI_NPM_TOKEN|npm\.pkg|\bnpm\s|\bnpx\s/,
  );
  assert.doesNotMatch(
    job,
    /--release\b|tauri\s+build|FONO_PREPARING_RELEASE_RESOURCES/,
  );
  assert.match(job, /run: node --test scripts\/ci-workflow\.test\.mjs/);
  assert.match(job, /run: \.\/scripts\/bootstrap-ci-resources\.ps1\r?\n/);
  assert.match(
    job,
    /\.\.\/scripts\/test-native\.ps1\r?\n\s+if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/,
  );
  assert.match(
    job,
    /cargo clippy --locked --workspace --all-targets -- -D warnings\r?\n\s+exit \$LASTEXITCODE/,
  );
  const config = JSON.parse(
    job.match(/^      TAURI_CONFIG: '([^']+)'$/m)?.[1] ?? "null",
  );
  assert.deepEqual(config, {
    build: { frontendDist: "../scripts/ci-native-frontend" },
    bundle: { resources: { "resources/stt-workers/*": null } },
  });
}

test("fork PR native tests have no frontend dependency, credentials or npm install", () => {
  checkNativeContract(workflow);
});

test("the native contract rejects regressions that skip forks or hide failing app tests", () => {
  checkNativeContract(workflow);
  const regressions = [
    ["frontend dependency", "    needs: frontend\n"],
    ["fork condition", "    if: github.event_name != 'pull_request'\n"],
    ["credentials", "    token: ${{ secrets.WHITEUI_NPM_TOKEN }}\n"],
    ["npm dependency", "    install: npm ci\n"],
  ];
  for (const [name, inserted] of regressions) {
    assert.throws(
      () =>
        checkNativeContract(
          workflow.replace("  native:\n", `  native:\n${inserted}`),
        ),
      undefined,
      name,
    );
  }
  for (const [before, after] of [
    ["../scripts/test-native.ps1", "cargo test --locked -p fono-core"],
    [
      "../scripts/test-native.ps1\n          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }",
      "../scripts/test-native.ps1",
    ],
    [
      "cargo clippy --locked --workspace --all-targets -- -D warnings",
      "cargo clippy --locked -p fono-core -- -D warnings",
    ],
    [
      '"frontendDist":"../scripts/ci-native-frontend"',
      '"frontendDist":"../dist"',
    ],
  ]) {
    assert.notEqual(workflow.indexOf(before), -1, `mutation target: ${before}`);
    assert.throws(() => checkNativeContract(workflow.replace(before, after)));
  }
});

test("the wrapper selects the whole workspace and the app remains a workspace member", async () => {
  const wrapper = await read("./test-native.ps1");
  const manifest = await read("../src-tauri/Cargo.toml");
  assert.match(
    wrapper,
    /\$CargoArguments = @\('--locked', '--workspace', '--all-targets'\)/,
  );
  assert.match(manifest, /members = \["\."[,\]]/);
});

test("native compile assets are inert and the real release frontend remains required", async () => {
  const fixture = await read("./ci-native-frontend/v3.html");
  assert.match(fixture, /Native CPU tests only/);
  assert.match(fixture, /default-src 'none'/);
  assert.doesNotMatch(fixture, /<script|\b(?:src|href)\s*=/i);
  assert.equal(tauriConfig.build.frontendDist, "../dist");
  assert.equal(
    tauriConfig.build.beforeBuildCommand,
    "npm run build && npm run prepare:release-resources",
  );
  assert.equal(
    tauriConfig.bundle.resources["resources/stt-workers/*"],
    "resources/stt-workers/",
  );
  assert.doesNotMatch(releaseWorkflow, /ci-native-frontend/);
  assert.match(
    releaseWorkflow,
    /node node_modules\/@tauri-apps\/cli\/tauri\.js build --ci/,
  );
});
