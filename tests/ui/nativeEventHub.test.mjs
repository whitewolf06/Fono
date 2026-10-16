import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, createEventHub, isBackendEventV1;
before(async () => {
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  ({ createEventHub } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/eventHub.ts",
  ));
  ({ isBackendEventV1 } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/backendEvents.ts",
  ));
});
after(async () => server?.close());

test("pending subscriptions share one native listen and release only the last owner", async () => {
  let receives,
    resolve,
    listens = 0,
    releases = 0;
  const hub = createEventHub(async (_, receive) => {
    listens++;
    receives = receive;
    return new Promise((done) => {
      resolve = done;
    });
  });
  const first = [],
    second = [];
  const a = hub.subscribe("backend-event-v1", (v) => first.push(v));
  const b = hub.subscribe("backend-event-v1", (v) => second.push(v));
  await Promise.resolve();
  assert.equal(listens, 1);
  resolve(() => releases++);
  const [releaseA, releaseB] = await Promise.all([a, b]);
  receives(1);
  releaseA();
  releaseA();
  receives(2);
  assert.deepEqual(first, [1]);
  assert.deepEqual(second, [1, 2]);
  assert.equal(releases, 0);
  releaseB();
  releaseB();
  assert.equal(releases, 1);
  assert.equal(hub.channelCount(), 0);
});

test("a rejected listen is removed and a later subscriber can retry", async () => {
  let listens = 0;
  const hub = createEventHub(async () => {
    if (++listens === 1) throw new Error("listen failed");
    return () => {};
  });
  const a = hub.subscribe("event", () => {});
  const b = hub.subscribe("event", () => {});
  await Promise.all([assert.rejects(a), assert.rejects(b)]);
  assert.equal(hub.channelCount(), 0);
  const release = await hub.subscribe("event", () => {});
  assert.equal(listens, 2);
  release();
});

test("a listener removed by another callback never receives that event", async () => {
  let emit, releaseB;
  const hub = createEventHub(async (_, receive) => {
    emit = receive;
    return () => {};
  });
  const releaseA = await hub.subscribe("event", () => releaseB());
  let count = 0;
  releaseB = await hub.subscribe("event", () => count++);
  emit(null);
  assert.equal(count, 0);
  releaseA();
});

test("backend envelope accepts only known V1 kinds", () => {
  assert.equal(
    isBackendEventV1({ schema_version: 1, kind: "service", payload: null }),
    true,
  );
  for (const value of [
    null,
    {},
    { schema_version: 2, kind: "service", payload: null },
    { schema_version: 1, kind: "future", payload: {} },
  ])
    assert.equal(isBackendEventV1(value), false);
});
