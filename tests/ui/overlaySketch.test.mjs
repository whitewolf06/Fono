import assert from "node:assert/strict";
import { before, after, afterEach, test } from "node:test";
import { createServer } from "vite";

let server, useOverlaySketch;
const sketches = [];
const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ useOverlaySketch } = await server.ssrLoadModule(
    "/src/v3/features/overlay/application/useOverlaySketch.ts",
  ));
});
afterEach(() => {
  sketches.splice(0).forEach((sketch) => sketch.dispose());
});
after(async () => server?.close());
function setup(patch = {}) {
  const copied = [],
    inserted = [],
    waits = [];
  let tick,
    unsubscribed = false;
  const sketch = useOverlaySketch({
    copy: async (text) => copied.push(text),
    onInsert: (text) => inserted.push(text),
    subscribeTick(callback, intervalMs) {
      assert.equal(intervalMs, 120);
      tick = callback;
      return () => {
        unsubscribed = true;
      };
    },
    wait(milliseconds) {
      const next = deferred();
      waits.push({ milliseconds, ...next });
      return next.promise;
    },
    ...patch,
  });
  sketches.push(sketch);
  return {
    sketch,
    copied,
    inserted,
    waits,
    tick: () => tick(),
    unsubscribed: () => unsubscribed,
  };
}
async function complete(waits) {
  waits[0].resolve();
  await flush();
  waits[1]?.resolve();
  await flush();
}

test("button finish retains result without insertion; successful Copy closes", async () => {
  const { sketch, inserted, copied, waits } = setup();
  const finishing = sketch.finish("button");
  assert.equal(sketch.state.phase, "transcribing");
  await complete(waits);
  await finishing;
  assert.equal(sketch.state.phase, "ready");
  assert.ok(sketch.state.result);
  assert.ok(sketch.state.originalText);
  assert.deepEqual(inserted, []);
  const result = sketch.state.result;
  await sketch.copy();
  assert.deepEqual(copied, [result]);
  assert.equal(sketch.state.phase, "closed");
  assert.deepEqual(inserted, []);
});

test("hotkey finish inserts demo text exactly once and closes", async () => {
  const { sketch, inserted, waits } = setup();
  const first = sketch.finish("hotkey");
  await sketch.finish("hotkey");
  await complete(waits);
  await first;
  assert.equal(inserted.length, 1);
  assert.equal(sketch.state.insertedText, inserted[0]);
  assert.equal(sketch.state.phase, "closed");
  assert.equal(waits.length, 2);
});

test("a manual session after hotkey insertion never inherits the previous inserted status", async () => {
  for (const closingAction of ["copy", "close"]) {
    const { sketch, inserted, copied, waits } = setup();
    const hotkey = sketch.finish("hotkey");
    await complete(waits);
    await hotkey;
    assert.ok(sketch.state.insertedText);
    assert.equal(inserted.length, 1);

    sketch.start();
    assert.equal(sketch.state.insertedText, "");
    const manual = sketch.finish("button");
    waits[2].resolve();
    await flush();
    waits[3].resolve();
    await manual;
    assert.equal(sketch.state.phase, "ready");
    assert.equal(sketch.state.insertedText, "");
    assert.equal(inserted.length, 1);
    const result = sketch.state.result;

    await sketch[closingAction]();
    assert.equal(sketch.state.phase, "closed");
    assert.equal(sketch.state.insertedText, "");
    assert.equal(inserted.length, 1);
    assert.deepEqual(copied, closingAction === "copy" ? [result] : []);
  }
});

test("latest choices are frozen at Finish and remembered after restart", async () => {
  const { sketch, waits } = setup();
  sketch.choose({ style: "formal", translationOn: true, language: "de" });
  const finishing = sketch.finish();
  sketch.choose({ style: "task", language: "fr" });
  await complete(waits);
  await finishing;
  assert.match(sketch.state.result, /Деловое письмо/);
  assert.match(sketch.state.result, /перевод · DE/);
  assert.equal(sketch.state.frozenChoice.style, "formal");
  sketch.start();
  assert.equal(sketch.state.style, "formal");
  assert.equal(sketch.state.translationOn, true);
  assert.equal(sketch.state.language, "de");
  assert.equal(sketch.state.result, "");
});

test("processing Off preserves translation choice but performs no processing or translation", async () => {
  const { sketch, waits } = setup({
    initial: { postprocessingOn: false, translationOn: true, language: "en" },
  });
  const finishing = sketch.finish();
  await complete(waits);
  await finishing;
  assert.equal(waits.length, 1);
  assert.equal(sketch.state.result, sketch.state.originalText);
  assert.doesNotMatch(sketch.state.result, /Демонстрационный перевод/);
  assert.equal(sketch.state.translationOn, true);
  sketch.start();
  sketch.choose({ postprocessingOn: true });
  assert.equal(sketch.state.translationOn, true);
});

test("failed Copy keeps ready result and can be retried without insertion", async () => {
  let failed = true;
  const copied = [];
  const { sketch, inserted, waits } = setup({
    copy: async (text) => {
      if (failed) throw new Error("Clipboard denied");
      copied.push(text);
    },
  });
  const finishing = sketch.finish();
  await complete(waits);
  await finishing;
  const result = sketch.state.result;
  await sketch.copy();
  assert.equal(sketch.state.phase, "ready");
  assert.equal(sketch.state.result, result);
  assert.match(sketch.state.error, /скопировать/);
  assert.equal(sketch.state.copying, false);
  failed = false;
  await sketch.copy();
  assert.deepEqual(copied, [result]);
  assert.equal(sketch.state.phase, "closed");
  assert.deepEqual(inserted, []);
});

test("Cancel and restart fence old recognition and processing completions", async () => {
  for (const duringProcessing of [false, true]) {
    const { sketch, waits, inserted } = setup();
    const old = sketch.finish("hotkey");
    if (duringProcessing) {
      waits[0].resolve();
      await flush();
      assert.equal(sketch.state.phase, "processing");
    }
    sketch.cancel();
    assert.equal(sketch.state.phase, "closed");
    assert.equal(sketch.state.result, "");
    sketch.start();
    const currentId = sketch.state.sessionId;
    waits.at(-1).resolve();
    await old;
    assert.equal(sketch.state.sessionId, currentId);
    assert.equal(sketch.state.phase, "recording");
    assert.equal(sketch.state.result, "");
    assert.deepEqual(inserted, []);
  }
});

test("late Copy completion cannot close a new recording; duplicate Copy is ignored", async () => {
  const clipboard = deferred();
  let count = 0;
  const { sketch, waits } = setup({
    copy: () => {
      count++;
      return clipboard.promise;
    },
  });
  const finishing = sketch.finish();
  await complete(waits);
  await finishing;
  const copying = sketch.copy();
  await sketch.copy();
  assert.equal(count, 1);
  sketch.close();
  sketch.start();
  clipboard.resolve();
  await copying;
  assert.equal(sketch.state.phase, "recording");
  assert.equal(sketch.state.copying, false);
});

test("timer and voice react only during recording and Dispose prevents late work", async () => {
  const { sketch, tick, waits, unsubscribed, inserted } = setup();
  tick();
  const firstLevel = sketch.state.level;
  tick();
  assert.equal(sketch.state.elapsedMs, 240);
  assert.notEqual(sketch.state.level, firstLevel);
  assert.ok(sketch.state.level >= 0 && sketch.state.level <= 1);
  const finishing = sketch.finish("hotkey");
  tick();
  assert.equal(sketch.state.elapsedMs, 240);
  assert.equal(sketch.state.level, 0);
  sketch.dispose();
  assert.equal(unsubscribed(), true);
  waits[0].resolve();
  await finishing;
  sketch.start();
  tick();
  assert.equal(sketch.state.elapsedMs, 240);
  assert.deepEqual(inserted, []);
});

test("failed demo insertion retains completed text for Copy; Close never inserts", async () => {
  const { sketch, copied, waits } = setup({
    onInsert: () => {
      throw new Error("Unavailable demo field");
    },
  });
  const finishing = sketch.finish("hotkey");
  await complete(waits);
  await finishing;
  assert.equal(sketch.state.phase, "error");
  assert.ok(sketch.state.result);
  assert.match(sketch.state.error, /скопировать/);
  const result = sketch.state.result;
  await sketch.copy();
  assert.deepEqual(copied, [result]);
  assert.equal(sketch.state.phase, "closed");
  assert.equal(sketch.state.insertedText, "");
});

test("failed processing retains original text for Copy and never inserts", async () => {
  const { sketch, copied, inserted, waits } = setup();
  const finishing = sketch.finish("hotkey");
  waits[0].resolve();
  await flush();
  waits[1].reject(new Error("Processing unavailable"));
  await finishing;
  assert.equal(sketch.state.phase, "error");
  assert.equal(sketch.state.result, sketch.state.originalText);
  assert.ok(sketch.state.result);
  assert.match(sketch.state.error, /Исходный текст/);
  assert.deepEqual(inserted, []);
  const original = sketch.state.originalText;
  await sketch.copy();
  assert.deepEqual(copied, [original]);
  assert.equal(sketch.state.phase, "closed");
});
