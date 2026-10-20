import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import {
  derive,
  pcm,
  summarize,
  decodedPcmHash,
} from "./long-dictation-quality.mjs";
import { hash } from "./stt-quality.mjs";

test("long WAV preserves consecutive whole public clips and exactly 60 seconds", (t) => {
  fs.mkdirSync(path.resolve("build"), { recursive: true });
  const directory = fs.mkdtempSync(path.resolve("build/fono124-corpus-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const root = path.resolve("src-tauri/tests/fixtures/stt-quality");
  const source = JSON.parse(fs.readFileSync(path.join(root, "manifest.json")));
  const corpus = derive(source, directory);
  for (const entry of corpus.entries) {
    const data = pcm(fs.readFileSync(path.join(directory, entry.path)));
    assert.equal(data.length, 60 * 16000 * 2);
    const candidates = source.entries.filter(
      (value) => value.language === entry.language,
    );
    assert.deepEqual(
      entry.sources.map((value) => value.id),
      candidates.slice(0, entry.sources.length).map((value) => value.id),
    );
    assert.equal(
      entry.reference,
      candidates
        .slice(0, entry.sources.length)
        .map((value) => value.reference)
        .join(" "),
    );
    for (const clip of entry.sources) {
      const original = candidates.find((value) => value.id === clip.id);
      assert.deepEqual(
        data.subarray(clip.start * 2, clip.end * 2),
        pcm(fs.readFileSync(path.join(root, original.path))),
      );
    }
    assert.equal(entry.sources.at(-1).end, 60 * 16000 - 1600);
  }
});
function pair() {
  const corpus = {
    entries: ["ru", "en"].map((language) => ({
      id: language,
      pcm_sha256: language,
      decoded_pcm_sha256: language,
      samples: 960000,
      reference: "a b c",
    })),
  };
  const run = {
    completed: true,
    device: "CPU",
    entries: corpus.entries.map((entry) => ({
      ...entry,
      baseline: { device: "CPU", text: "a b c", stop_ready_seconds: 2 },
      capture: {
        device: "CPU",
        text: "a b c",
        stop_ready_seconds: 1,
        feed_seconds: 60,
        processed_until: 800000,
        prefix: [{ start: 0, end: 800000 }],
        tail: [{ start: 800000, end: 960000 }],
        full_batch_fallback: false,
      },
      service_scheduler: { entered_before_interactive_finish: false },
    })),
    cancel: {
      recognizer_active_at_cancel: true,
      active_inferences_after_join: 0,
      scheduler_busy_after_join: false,
      activity_busy_after_cleanup: false,
    },
  };
  return { run, corpus };
}
test("paired quality has zero WER margin and cannot hide one language's regression", () => {
  const { run, corpus } = pair();
  assert.equal(
    summarize(run, corpus).acceptance.wer_not_worse_each_language,
    true,
  );
  run.entries[0].capture.text = "a b";
  run.entries[1].baseline.text = "wrong words here";
  assert.equal(
    summarize(run, corpus).acceptance.wer_not_worse_each_language,
    false,
  );
});
test("expected decoded hash includes production PCM16 float conversion at peaks", () => {
  const source = Buffer.alloc(10);
  [-32768, -1, 0, 1, 32767].forEach((value, index) =>
    source.writeInt16LE(value, index * 2),
  );
  const expected = Buffer.alloc(10);
  [-32767, -1, 0, 1, 32766].forEach((value, index) =>
    expected.writeInt16LE(value, index * 2),
  );
  assert.equal(decodedPcmHash(source), hash(expected));
  assert.notEqual(decodedPcmHash(source), hash(source));
});
test("different PCM/device, a gap and unfinished cancel invalidate measurement", () => {
  for (const change of [
    (run) => {
      run.entries[0].pcm_sha256 = "different";
    },
    (run) => {
      run.entries[0].capture.device = "Vulkan";
    },
    (run) => {
      run.entries[0].capture.tail[0].start++;
    },
    (run) => {
      run.cancel.active_inferences_after_join = 1;
    },
  ]) {
    const { run, corpus } = pair();
    change(run);
    assert.throws(() => summarize(run, corpus));
  }
});
test("whole-batch fallback is recorded without claiming prefix reuse", () => {
  const { run, corpus } = pair();
  run.entries[0].capture.full_batch_fallback = true;
  assert.equal(
    summarize(run, corpus).acceptance.prefix_reused_each_language,
    false,
  );
});
