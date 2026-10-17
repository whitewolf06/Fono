import assert from "node:assert/strict";
import { createHash } from "node:crypto";
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

function frontendJob(source) {
  const jobs = [...source.matchAll(/^ {2}([a-z][\w-]*):\r?\n/gm)];
  const index = jobs.findIndex((match) => match[1] === "frontend");
  assert.notEqual(index, -1, "frontend job must exist");
  return source
    .slice(jobs[index].index, jobs[index + 1]?.index)
    .replace(/^\s*#.*$/gm, "");
}

function checkFrontendContract(source) {
  const job = frontendJob(source);
  assert.doesNotMatch(
    source,
    /pull_request_target|secrets\.|WHITEUI_NPM_TOKEN/,
  );
  assert.match(job, /^ {4}runs-on: ubuntu-24\.04$/m);
  assert.doesNotMatch(job, /^ {4}(?:if|needs|environment):/m);
  assert.doesNotMatch(job, /NODE_AUTH_TOKEN|npm\.pkg\.github\.com/);
  assert.match(job, /persist-credentials: false/);
  assert.match(job, /registry-url: https:\/\/registry\.npmjs\.org/);
  assert.match(job, /\bnpm ci\b/);
  for (const gate of [
    "typecheck:v3",
    "lint",
    "test:v3",
    "test:release",
    "test:cpu",
    "docs:check",
    "format:check",
    "version:check",
    "build",
  ]) {
    assert.ok(
      job.includes(`npm run ${gate}`),
      `missing frontend gate: ${gate}`,
    );
  }
}

test("frontend checks run anonymously for external fork pull requests", () => {
  checkFrontendContract(workflow);
  for (const inserted of [
    "    if: github.event.pull_request.head.repo.full_name == github.repository\n",
    "    needs: private-bootstrap\n",
    "    token: ${{ secrets.WHITEUI_NPM_TOKEN }}\n",
  ]) {
    assert.throws(() =>
      checkFrontendContract(
        workflow.replace("  frontend:\n", `  frontend:\n${inserted}`),
      ),
    );
  }
});

test("WhiteUI uses the exact original local 0.6.0 archive and public dependency URLs", async () => {
  const manifest = JSON.parse(await read("../package.json"));
  const lock = JSON.parse(await read("../package-lock.json"));
  const provenance = JSON.parse(
    await read("../vendor/whiteui/provenance.json"),
  );
  const spec = "file:vendor/whiteui/whitelife-core-ui-kit-0.6.0.tgz";
  const archive = await readFile(
    new URL(`../${spec.slice(5)}`, import.meta.url),
  );
  const packageEntry = lock.packages["node_modules/@whitelife-core/ui-kit"];
  assert.equal(manifest.dependencies["@whitelife-core/ui-kit"], spec);
  assert.equal(lock.packages[""].dependencies["@whitelife-core/ui-kit"], spec);
  assert.equal(packageEntry.version, "0.6.0");
  assert.equal(packageEntry.resolved, spec);
  assert.equal(packageEntry.integrity, provenance.original.integrity);
  assert.equal(archive.length, 134150);
  assert.equal(
    createHash("sha256").update(archive).digest("hex"),
    "e0aabc07070a54eae56cfb810f120b3ebdcb12f34e1313a1eecc073c5dfaab65",
  );
  assert.equal(
    provenance.original.sha256,
    createHash("sha256").update(archive).digest("hex"),
  );
  assert.equal(
    `sha512-${createHash("sha512").update(archive).digest("base64")}`,
    packageEntry.integrity,
  );
  assert.equal(
    (await read("../.npmrc")).trim(),
    "registry=https://registry.npmjs.org/",
  );
  for (const entry of Object.values(lock.packages)) {
    if (!entry.resolved || entry.resolved === spec) continue;
    const url = new URL(entry.resolved);
    assert.equal(url.protocol, "https:");
    assert.equal(url.hostname, "registry.npmjs.org");
    assert.equal(url.username, "");
    assert.equal(url.password, "");
    assert.equal(url.search, "");
  }
  assert.doesNotMatch(
    releaseWorkflow,
    /WHITEUI_NPM_TOKEN|NODE_AUTH_TOKEN|npm\.pkg\.github/,
  );
  assert.match(
    releaseWorkflow,
    /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY \}\}/,
  );
});

// This checks the deliberately small job contract without an npm YAML parser:
// the test itself must run in an empty checkout before installing dependencies.
function nativeJob(source) {
  const jobs = [...source.matchAll(/^ {2}([a-z][\w-]*):\r?\n/gm)];
  const index = jobs.findIndex((match) => match[1] === "native");
  assert.notEqual(index, -1, "native job must exist");
  return source
    .slice(jobs[index].index, jobs[index + 1]?.index)
    .replace(/^\s*#.*$/gm, "");
}

function checkNativeContract(source) {
  const job = nativeJob(source);
  assert.match(source, /^ {2}pull_request:\r?\n {4}branches: \[main\]/m);
  assert.doesNotMatch(source, /^ {2}pull_request_target:/m);
  assert.doesNotMatch(
    source.split(/^jobs:/m)[0],
    /secrets\.|NODE_AUTH_TOKEN|WHITEUI_NPM_TOKEN/,
  );
  assert.match(job, /^ {4}runs-on: windows-2022$/m);
  assert.doesNotMatch(job, /^\s+(?:needs|if):/m);
  assert.doesNotMatch(job, /^ {4}(?:environment|uses):/m);
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
  assert.match(job, /run: python scripts\/cuda-package\.test\.py\r?\n/);
  assert.match(job, /run: node --test scripts\/dev-cpu\.test\.mjs/);
  const bootstrap = job.indexOf("run: ./scripts/bootstrap-ci-resources.ps1\n");
  const cudaPackaging = job.indexOf(
    "run: python scripts/cuda-package.test.py\n",
  );
  const launcher = job.indexOf("run: node --test scripts/dev-cpu.test.mjs");
  assert.ok(
    cudaPackaging > bootstrap,
    "CUDA package tests require bootstrapped Python",
  );
  assert.ok(
    launcher > cudaPackaging,
    "CUDA package tests must precede launcher/native tests",
  );
  assert.ok(
    job.indexOf("../scripts/test-native.ps1") > launcher,
    "Native workspace tests must follow packaging and launcher gates",
  );
  assert.match(
    job,
    /\.\.\/scripts\/test-native\.ps1\r?\n\s+if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/,
  );
  assert.match(
    job,
    /cargo clippy --locked --workspace --all-targets -- -D warnings\r?\n\s+exit \$LASTEXITCODE/,
  );
  const config = JSON.parse(
    job.match(/^ {6}TAURI_CONFIG: '([^']+)'$/m)?.[1] ?? "null",
  );
  assert.deepEqual(config, {
    build: { frontendDist: "../scripts/ci-native-frontend" },
    bundle: {
      resources: { "resources/stt-base/fono-stt-vulkan-worker.exe": null },
    },
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
    ["python scripts/cuda-package.test.py", "python --version"],
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
    tauriConfig.bundle.resources[
      "resources/stt-base/fono-stt-vulkan-worker.exe"
    ],
    "resources/stt-workers/fono-stt-vulkan-worker.exe",
  );
  assert.doesNotMatch(releaseWorkflow, /ci-native-frontend/);
  assert.match(
    releaseWorkflow,
    /node node_modules\/@tauri-apps\/cli\/tauri\.js build --ci/,
  );
});

test("native checks keep default WakeWord absent and cover independent live/wake feature combinations", () => {
  const job = nativeJob(workflow);
  assert.match(job, /cargo tree --locked -p fono --edges normal/);
  assert.match(job, /\$defaultTree -match '\\bfono-wake v'/);
  const matrix = job.match(/foreach \(\$features in @\(([^)]+)\)\)/)?.[1];
  assert.ok(matrix, "feature matrix must be explicit");
  assert.deepEqual(
    [...matrix.matchAll(/'([^']*)'/g)].map((match) => match[1]),
    [
      "",
      "wake-word",
      "sherpa-wake",
      "experimental-live",
      "experimental-live,sherpa-wake",
    ],
  );
  assert.match(
    job,
    /\$arguments = @\('check', '--locked', '-p', 'fono', '--no-default-features'\)/,
  );
  assert.match(
    job,
    /if \(\$features\) \{ \$arguments \+= @\('--features', \$features\) \}/,
  );
  assert.match(
    job,
    /& cargo @arguments\r?\n\s+if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/,
  );
});
