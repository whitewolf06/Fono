import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const paths = [
  "package.json",
  "package-lock.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

function parseVersion(version) {
  const match = semver.exec(version);
  if (!match) throw new Error(`Некорректная версия: ${version}`);
  return match.slice(1).map(Number);
}

function readVersion(file, content) {
  if (file === "package.json" || file === "src-tauri/tauri.conf.json") {
    return JSON.parse(content).version;
  }
  if (file === "package-lock.json") {
    const lock = JSON.parse(content);
    if (lock.version !== lock.packages[""].version) {
      throw new Error("Версии корня package-lock.json различаются");
    }
    return lock.version;
  }
  if (file === "src-tauri/Cargo.toml") {
    return content.match(
      /^\[package\]\r?\n(?:[^[]*?\r?\n)*?version = "([^"]+)"/m,
    )?.[1];
  }
  return content.match(/^name = "fono"\r?\nversion = "([^"]+)"/m)?.[1];
}

function fileContents(source) {
  return Object.fromEntries(paths.map((file) => [file, source(file)]));
}

function assertSynchronized(contents) {
  const expected = readVersion("package.json", contents["package.json"]);
  parseVersion(expected);
  for (const file of paths) {
    const actual = readVersion(file, contents[file]);
    if (actual !== expected) {
      throw new Error(
        `${file}: ожидается ${expected}, найдено ${actual ?? "ничего"}`,
      );
    }
  }
  return expected;
}

function replaceVersion(file, content, version) {
  if (file === "package-lock.json") {
    const lock = JSON.parse(content);
    lock.version = version;
    lock.packages[""].version = version;
    return `${JSON.stringify(lock, null, 2)}\n`;
  }
  if (file === "src-tauri/Cargo.lock") {
    return content.replace(
      /^(name = "fono"\r?\nversion = ")[^"]+("\r?$)/m,
      `$1${version}$2`,
    );
  }
  if (file === "src-tauri/Cargo.toml") {
    return content.replace(
      /^(\[package\]\r?\n(?:[^[]*?\r?\n)*?version = ")[^"]+("\r?$)/m,
      `$1${version}$2`,
    );
  }
  return content.replace(
    /^(\s*"version": ")[^"]+("[,]?\r?$)/m,
    `$1${version}$2`,
  );
}

function gitShow(spec) {
  return execFileSync("git", ["show", spec], { cwd: root, encoding: "utf8" });
}

function checkCommit(messageFile) {
  if (!messageFile) throw new Error("Укажите файл сообщения коммита");
  const staged = fileContents((file) => gitShow(`:${file}`));
  const next = assertSynchronized(staged);
  const previous = readVersion("package.json", gitShow("HEAD:package.json"));
  const [oldMajor, oldMinor, oldPatch] = parseVersion(previous);
  const [major, minor, patch] = parseVersion(next);
  const patchBump =
    major === oldMajor && minor === oldMinor && patch === oldPatch + 1;
  const minorBump = major === oldMajor && minor === oldMinor + 1 && patch === 0;
  const initialBaseline = previous === "0.1.1" && next === "0.5.0";
  if (!patchBump && !minorBump && !initialBaseline) {
    throw new Error(
      `Коммит должен поднять patch на 1 или согласованный minor: ${previous} → ${next}`,
    );
  }
  const subject = readFileSync(messageFile, "utf8").split(/\r?\n/, 1)[0];
  if (
    !new RegExp(`(?:^|[^0-9])v${next.replaceAll(".", "\\.")}(?![0-9])`).test(
      subject,
    )
  ) {
    throw new Error(`Укажите v${next} в теме коммита`);
  }
  console.log(`Версия коммита подтверждена: ${previous} → ${next}`);
}

try {
  const [command, argument] = process.argv.slice(2);
  if (command === "commit-check") {
    checkCommit(argument);
  } else {
    const contents = fileContents((file) =>
      readFileSync(path.join(root, file), "utf8"),
    );
    const current = assertSynchronized(contents);
    if (command === "check") {
      console.log(`Версия Fono: ${current}`);
    } else if (command === "set" || command === "bump") {
      const [major, minor, patch] = parseVersion(current);
      const next =
        command === "set"
          ? argument
          : argument === "patch"
            ? `${major}.${minor}.${patch + 1}`
            : argument === "minor"
              ? `${major}.${minor + 1}.0`
              : undefined;
      parseVersion(next);
      if (next === current) throw new Error("Версия не изменилась");
      for (const file of paths) {
        writeFileSync(
          path.join(root, file),
          replaceVersion(file, contents[file], next),
        );
      }
      console.log(`Версия Fono: ${current} → ${next}`);
    } else {
      throw new Error(
        "Использование: node scripts/version.mjs check | bump patch | bump minor | set X.Y.Z",
      );
    }
  }
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
