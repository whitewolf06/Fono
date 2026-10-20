// The same process contract runs against both shipped worker implementations.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const protocol = fs.readFileSync(
  path.join(root, "src-tauri/crates/fono-stt-protocol/src/lib.rs"),
  "utf8",
);
const version = Number(
  protocol.match(/PROTOCOL_VERSION:\s*u16\s*=\s*(\d+)/)?.[1],
);
const requestLimit = 16 * 1024 * 1024;
const responseLimit = 1024 * 1024;

export async function workerContract(executable, backend) {
  assert.ok(["cuda", "vulkan", "cpu"].includes(backend));
  const child = spawn(path.resolve(executable), [], {
    cwd: path.dirname(path.resolve(executable)),
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  let buffered = Buffer.alloc(0),
    waiting,
    failure;
  const responses = [],
    cases = [];
  const fail = (error) => {
    failure ??= error;
    if (waiting) {
      waiting.reject(failure);
      waiting = undefined;
    }
  };
  child.on("error", fail);
  child.stdin.on("error", fail);
  child.stderr.resume();
  child.stdout.on("data", (chunk) => {
    buffered = Buffer.concat([buffered, chunk]);
    let newline;
    while ((newline = buffered.indexOf(10)) >= 0) {
      if (newline + 1 > responseLimit)
        return fail(new Error("response exceeds wire limit"));
      try {
        const response = JSON.parse(
          buffered.subarray(0, newline).toString("utf8"),
        );
        if (waiting) {
          waiting.resolve(response);
          waiting = undefined;
        } else responses.push(response);
      } catch (error) {
        return fail(error);
      }
      buffered = buffered.subarray(newline + 1);
    }
    if (buffered.length > responseLimit)
      fail(new Error("unterminated response exceeds limit"));
  });
  const exited = new Promise((resolve) =>
    child.once("close", (code, signal) => resolve({ code, signal })),
  );
  const take = () => {
    if (failure) return Promise.reject(failure);
    if (responses.length) return Promise.resolve(responses.shift());
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        waiting = undefined;
        reject(new Error(`${backend} worker response timed out`));
      }, 10_000);
      waiting = {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      };
    });
  };
  const send = async (request, expected, code) => {
    const result = take();
    child.stdin.write(
      typeof request === "string" ? request : `${JSON.stringify(request)}\n`,
    );
    const response = await result;
    assert.equal(response.type, expected);
    assert.equal(response.protocol_version, version);
    if (typeof request !== "string")
      assert.equal(response.request_id, request.request_id);
    if (code) assert.equal(response.code, code);
    return response;
  };
  const envelope = (type, id) => ({
    type,
    protocol_version: version,
    request_id: id,
  });
  try {
    const hello = await send(envelope("hello", "hello"), "ready");
    assert.equal(hello.backend, backend);
    assert.equal(hello.capabilities.protocol_version, version);
    for (const key of [
      "supports_health",
      "supports_shutdown",
      "supports_window",
      "supports_cancel",
      "supports_token_timestamps",
      "supports_decoding_options",
    ])
      assert.equal(hello.capabilities[key], true, key);
    assert.equal(hello.capabilities.maximum_request_bytes, requestLimit);
    assert.equal(hello.capabilities.maximum_response_bytes, responseLimit);
    cases.push("handshake_and_capabilities");
    await send(envelope("ping", "health"), "pong");
    cases.push("health");
    await send(
      { ...envelope("ping", "wrong-version"), protocol_version: version + 1 },
      "error",
      "protocol_version",
    );
    cases.push("version_rejected");
    for (const audio of ["AQ==", "!"]) {
      const request = {
        ...envelope("transcribe", `pcm-${cases.length}`),
        operation_id: "contract-operation",
        model_path: "",
        language: "auto",
        samples_i16_base64: audio,
      };
      const error = await send(request, "error", "audio");
      assert.equal(error.operation_id, request.operation_id);
      cases.push(
        audio === "AQ==" ? "odd_pcm_rejected" : "invalid_base64_rejected",
      );
    }
    const inference = {
      ...envelope("transcribe", "decoding-accurate"),
      operation_id: "contract-operation",
      model_path: "",
      language: "auto",
      samples_i16_base64: "!",
      decoding: { strategy: "accurate", threads: 3 },
    };
    await send(inference, "error", "audio");
    cases.push("accurate_options_accepted_before_pcm_validation");
    for (const threads of [0, 9]) {
      await send(
        {
          ...inference,
          request_id: `threads-${threads}`,
          samples_i16_base64: "AAA=",
          decoding: { strategy: "fast", threads },
        },
        "error",
        "decoding",
      );
      cases.push(`threads_${threads}_rejected_before_model_load`);
    }
    for (const glossary of [
      ["private\nterm"],
      Array(33).fill("term"),
      ["x".repeat(897)],
    ]) {
      await send(
        { ...inference, request_id: `glossary-${cases.length}`, glossary },
        "error",
        "prompt",
      );
      cases.push("invalid_glossary_rejected_before_model_load");
    }
    child.stdin.write(
      `${JSON.stringify({ ...envelope("cancel_request", "cancel-idle"), target_request_id: "missing" })}\n`,
    );
    await send(envelope("ping", "after-cancel"), "pong");
    cases.push("cancel_control_has_no_reply");
    await send("{invalid}\n", "error", "request_json");
    cases.push("malformed_json_rejected");
    // The wire limit includes LF, consistently with the Rust bounded reader.
    await send(`${" ".repeat(requestLimit - 1)}\n`, "error", "request_json");
    cases.push("maximum_wire_frame_accepted_by_transport");
    await send(`${" ".repeat(requestLimit)}\n`, "error", "request_frame");
    cases.push("oversized_wire_frame_rejected");
    await send(envelope("ping", "after-frame-error"), "pong");
    cases.push("frame_recovery");
    await send(envelope("shutdown", "shutdown"), "shutting_down");
    child.stdin.end();
    const terminal = await Promise.race([
      exited,
      new Promise((_, reject) => {
        const timer = setTimeout(
          () => reject(new Error("worker did not exit after shutdown")),
          3_000,
        );
        timer.unref();
      }),
    ]);
    assert.deepEqual(terminal, { code: 0, signal: null });
    cases.push("graceful_shutdown");
    return {
      backend,
      protocol_version: version,
      cases,
      passed: true,
      scope:
        "process framing/control; no model load, inference, microphone or GPU quality acceptance",
    };
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await exited;
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  workerContract(process.argv[2], process.argv[3])
    .then((report) => {
      console.log(JSON.stringify(report));
    })
    .catch((error) => {
      console.error(error.message);
      process.exitCode = 1;
    });
}
