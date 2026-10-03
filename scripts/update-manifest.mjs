import { mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import {
  argumentsMap,
  fileDigest,
  httpsUrl,
  MAX_SIGNATURE_BYTES,
  readBounded,
  validateManifest,
  verifyInstaller,
  updateChannel,
} from "./release-support.mjs";

try {
  const options = argumentsMap(process.argv.slice(2));
  const allowed = [
    "installer",
    "signature",
    "download-base",
    "output",
    "notes",
    "date",
  ];
  if (
    !options.installer ||
    !options["download-base"] ||
    !options.output ||
    Object.keys(options).some((key) => !allowed.includes(key))
  ) {
    throw new Error(
      "Usage: node scripts/update-manifest.mjs --installer path --download-base https://host/version/ --output build/latest.json [--signature path] [--notes path] [--date UTC-RFC3339]",
    );
  }
  const installer = resolve(options.installer);
  if (!basename(installer).endsWith(".exe"))
    throw new Error("Expected an NSIS .exe installer");
  const signaturePath = resolve(options.signature ?? `${installer}.sig`);
  const signature = (await readBounded(signaturePath, MAX_SIGNATURE_BYTES))
    .toString("utf8")
    .trim();
  const { version } = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  );
  const { publicKey } = await updateChannel();
  await verifyInstaller(installer, signature, publicKey, version);
  const base = httpsUrl(options["download-base"], "Download base");
  if (!base.pathname.endsWith("/")) base.pathname += "/";
  const url = new URL(encodeURIComponent(basename(installer)), base).href;
  if (basename(installer) !== `Fono_${version}_x64-setup.exe`) {
    throw new Error(
      "Installer filename does not match the current Fono version and architecture",
    );
  }
  const notes = options.notes
    ? (await readBounded(resolve(options.notes), 16000)).toString("utf8")
    : "";
  const manifest = validateManifest(
    {
      version,
      notes,
      pub_date: options.date ?? new Date().toISOString(),
      platforms: { "windows-x86_64": { url, signature } },
    },
    version,
  );
  const output = resolve(options.output);
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  const digest = (await fileDigest(installer)).toString("hex");
  await writeFile(
    `${output}.sha256`,
    `${digest}  ${basename(installer)}\n`,
    "utf8",
  );
  console.log(
    `Verified NSIS signature and generated ${basename(output)} for Fono ${version}. No upload performed.`,
  );
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
