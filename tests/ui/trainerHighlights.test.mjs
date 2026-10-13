import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer as createTcpServer } from "node:net";
import { createServer } from "vite";
import { createSSRApp } from "vue";
import { renderToString } from "@vue/server-renderer";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

let server, highlights, analyze;
before(async () => {
  const socket = createTcpServer();
  await new Promise((resolve, reject) => {
    socket.once("error", reject);
    socket.listen(0, "127.0.0.1", resolve);
  });
  const port = socket.address().port;
  await new Promise((resolve) => socket.close(resolve));
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { hmr: { host: "127.0.0.1", port }, middlewareMode: true },
  });
  ({ transcriptHighlights: highlights } = await server.ssrLoadModule(
    "/src/v3/features/trainer/application/transcriptHighlights.ts",
  ));
  ({ analyze } = await server.ssrLoadModule(
    "/src/v3/features/trainer/domain/analysis.ts",
  ));
});
after(async () => server?.close());

const finding = (title, count, example) => ({ title, count, example });
const painted = (segments) =>
  segments.filter((part) => part.highlighted).map((part) => part.text);
const markers = (segments) => segments.flatMap((part) => part.markers || []);
const highlight = (original, findings) => {
  const result = highlights(original, findings);
  assert.equal(result.map((part) => part.text).join(""), original);
  return result;
};

test("saved markers use Unicode whole words rather than substrings or identifiers", () => {
  const original = "нужно нулевой ну42 42ну ну_ка ну́, НУ! 😀ну — café caféine";
  const parts = highlight(original, [
    finding("Слова-паразиты", 3, "ну, НУ, café"),
  ]);
  assert.deepEqual(painted(parts), ["НУ", "ну", "café"]);
});

test("saved phrase fragments match punctuation and line breaks without changing text", () => {
  const original = "Как,\n\tбы, это то — есть мысль. В   смысле, другое.";
  const parts = highlight(original, [
    finding("Слова-паразиты", 1, "как бы"),
    finding("Самоисправления", 2, "то есть, в смысле"),
  ]);
  assert.deepEqual(painted(parts), ["Как,\n\tбы", "то — есть", "В   смысле"]);
});

test("overlapping repeated pairs keep both numbered categories without duplicating any character", () => {
  const original = "Ну, ну ну. Затем потом, потом.";
  const parts = highlight(original, [
    finding("Слова-паразиты", 3, "Ну, ну, ну"),
    finding("Повторы", 3, "Ну ну, ну ну, потом потом"),
  ]);
  assert.equal(painted(parts).join(""), "Ну, ну нупотом, потом");
  assert.deepEqual(
    markers(parts).map((marker) => marker.number),
    [1, 2, 3, 4, 5, 6],
  );
  assert.equal(
    markers(parts).filter((marker) => marker.category === "filler").length,
    3,
  );
  assert.equal(
    markers(parts).filter((marker) => marker.category === "repetition").length,
    3,
  );
  assert.ok(parts.some((part) => part.category === "filler"));
  assert.ok(parts.some((part) => part.category === "repetition"));
});

test("all four graph categories keep their own color key and global occurrence number", () => {
  const parts = highlight("Ну, потом потом; то есть мысль…", [
    finding("Слова-паразиты", 1, "Ну"),
    finding("Повторы", 1, "потом потом"),
    finding("Самоисправления", 1, "то есть"),
    finding("Незавершённые фразы", 1, "мысль"),
  ]);
  assert.deepEqual(
    markers(parts).map((marker) => marker.category),
    ["filler", "repetition", "selfCorrection", "unfinished"],
  );
  assert.deepEqual(
    markers(parts).map((marker) => marker.number),
    [1, 2, 3, 4],
  );
  assert.deepEqual(
    parts.filter((part) => part.highlighted).map((part) => part.category),
    ["filler", "repetition", "selfCorrection", "unfinished"],
  );
});

test("limited examples never expand a saved count or mutate the saved findings", () => {
  const findings = [finding("Слова-паразиты", 12, "ну, ну, ну")];
  const saved = structuredClone(findings);
  const first = highlight("ну, ну, ну, ну, ну", findings);
  assert.equal(markers(first).length, 3);
  assert.deepEqual(findings, saved);
  assert.deepEqual(highlight("ну, ну, ну, ну, ну", findings), first);
});

test("a repetition paints only the saved adjacent pair, leaving isolated copies unchanged", () => {
  const parts = highlight("мысль, другая мысль; затем мысль мысль", [
    finding("Повторы", 1, "мысль мысль"),
  ]);
  assert.deepEqual(painted(parts), ["мысль мысль"]);
});

test("persisted zero or missing categories and absent examples never rerun local rules", () => {
  const original = "Ну ну, как бы, то есть...";
  const persisted = [
    finding("Слова-паразиты", 0, "Ну"),
    finding("Повторы", 1, "другое другое"),
  ];
  const authoritative = analyze({ original, findings: persisted });
  assert.equal(authoritative, persisted);
  assert.deepEqual(painted(highlight(original, authoritative)), []);
  assert.deepEqual(
    painted(highlight(original, [finding("Самоисправления", 1, "")])),
    [],
  );
});

test("saved example multiplicity and category count bound the number of painted occurrences", () => {
  const original = "ну, ну, ну";
  assert.deepEqual(
    painted(highlight(original, [finding("Слова-паразиты", 1, "ну, ну, ну")])),
    ["ну"],
  );
  assert.deepEqual(
    painted(highlight(original, [finding("Слова-паразиты", 3, "ну")])),
    ["ну"],
  );
  assert.deepEqual(
    painted(highlight(original, [finding("Слова-паразиты", 3, "ну, ну, ну")])),
    ["ну", "ну", "ну"],
  );
});

test("unfinished examples paint only the last matching word with an unfinished ending", () => {
  for (const suffix of ["…", "...", "-\n"]) {
    assert.deepEqual(
      painted(
        highlight(`мысль и ещё мысль${suffix}`, [
          finding("Незавершённые фразы", 1, "мысль"),
        ]),
      ),
      ["мысль"],
    );
  }
  assert.deepEqual(
    painted(
      highlight("мысль и ещё мысль.", [
        finding("Незавершённые фразы", 1, "мысль"),
      ]),
    ),
    [],
  );
});

test("saved spellings stay authoritative without expanding ё into unsaved е matches", () => {
  const parts = highlight("елка ёлка. Ёлка ЕЛКА.", [
    finding("Повторы", 1, "Ёлка ЕЛКА"),
  ]);
  assert.deepEqual(painted(parts), ["Ёлка ЕЛКА"]);
});

test("empty and clean originals preserve their exact value", () => {
  for (const original of ["", " \n\t", "Мысль выражена ясно."]) {
    assert.deepEqual(painted(highlight(original, [])), []);
  }
});

test("HTML-like content remains literal text around highlights and is never accepted as a pattern", () => {
  const original =
    '<img src=x onerror="alert(1)"> Ну <script>window.bad=true</script> & ну.';
  const parts = highlight(original, [finding("Слова-паразиты", 2, "Ну, ну")]);
  assert.deepEqual(painted(parts), ["Ну", "ну"]);
  assert.ok(
    parts.some((part) => !part.highlighted && part.text.includes("<img")),
  );
  assert.deepEqual(
    painted(
      highlight(original, [
        finding("Слова-паразиты", 1, '<img src=x onerror="alert(1)">'),
      ]),
    ),
    [],
  );
});

test("TrainerDetails renders exact escaped transcript text and marks without HTML injection", async () => {
  const { default: Details } = await server.ssrLoadModule(
    "/src/v3/features/trainer/presentation/TrainerDetails.vue",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const original =
    '  Ну,\nкак бы!\tну\r\n<img src=x onerror="alert(1)"> <script>bad()</script> &lt; 😀';
  const app = createSSRApp(Details, {
    entry: {
      id: "fixture",
      title: "Fixture",
      createdAt: "2026-10-13T12:00:00.000Z",
      duration: 2,
      text: "Result",
      original,
      findings: [
        {
          ...finding("Слова-паразиты", 12, "Ну, как бы, ну"),
          advice: "Fixture",
        },
      ],
    },
  });
  app.use(WlConfig);
  app.use(WlToastService);
  app.component("RouterLink", { render: () => null });
  app.provide(workspaceKey, {
    state: {
      preferences: { trainerAiEnabled: true, trainerModel: "fixture-model" },
    },
    native: false,
  });
  const html = await renderToString(app);
  const content = html.match(
    /class="trainer-original-text"[^>]*>([\s\S]*?)<\/div>/u,
  )?.[1];
  assert.ok(content);
  assert.match(content, /<mark[^>]*class="transcript-marker"/u);
  assert.match(content, /data-category="filler"/u);
  assert.equal(
    (content.match(/class="transcript-marker-number"/gu) || []).length,
    3,
  );
  assert.deepEqual(
    [...content.matchAll(/data-number="(\d+)"/gu)].map((match) =>
      Number(match[1]),
    ),
    [1, 2, 3],
  );
  assert.doesNotMatch(content, /<(?:img|script)\b/iu);
  assert.match(content, /&lt;img/u);
  const entities = { lt: "<", gt: ">", amp: "&", quot: '"', "#39": "'" };
  const visibleText = content
    .replace(/<[^>]*>/gu, "")
    .replace(/&(lt|gt|amp|quot|#39);/gu, (_, entity) => entities[entity]);
  assert.equal(visibleText, original);
  const counters = html.match(
    /class="trainer-category-counts"[^>]*>([\s\S]*?)<\/dl>/u,
  )?.[1];
  assert.ok(counters);
  assert.equal(
    (counters.match(/class="trainer-category-dot"/gu) || []).length,
    4,
  );
  assert.match(counters, /<dd[^>]*>12<\/dd>/u);
  assert.equal((counters.match(/<dd[^>]*>0<\/dd>/gu) || []).length, 3);
  assert.doesNotMatch(
    html,
    /Локальный разбор|Не найдены|Небольшая практика|Получить рекомендацию ИИ|Настроить рекомендации через ИИ/u,
  );
});
