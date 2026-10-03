import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import {
  argumentsMap,
  assertSigningCliVersion,
  readBounded,
  updateChannel,
} from "./release-support.mjs";

try {
  const options = argumentsMap(process.argv.slice(2));
  if (!options.output || Object.keys(options).some((key) => key !== "output")) {
    throw new Error(
      "Usage: node scripts/update-release-config.mjs --output build/update-release.conf.json",
    );
  }
  const cliPackage = JSON.parse(
    (
      await readBounded(
        new URL(
          "../node_modules/@tauri-apps/cli/package.json",
          import.meta.url,
        ),
        64000,
      )
    ).toString("utf8"),
  );
  assertSigningCliVersion(cliPackage.version);
  const { publicKey, endpoint } = await updateChannel();
  const output = resolve(options.output);
  const config = {
    bundle: { createUpdaterArtifacts: true, targets: ["nsis"] },
    plugins: {
      updater: {
        pubkey: publicKey,
        endpoints: [endpoint],
        requireSignedVersion: true,
        windows: { installMode: "passive" },
      },
    },
  };
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(config, null, 2)}\n`, "utf8");
  console.log(
    "Validated updater release override written; no network request or publication performed.",
  );
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
