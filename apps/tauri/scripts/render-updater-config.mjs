import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const publicKey = process.env.TAURI_UPDATER_PUBLIC_KEY;

if (!publicKey) {
  console.error("TAURI_UPDATER_PUBLIC_KEY is required to render the updater config.");
  process.exit(1);
}

const scriptDir = dirname(fileURLToPath(import.meta.url));
const outputPath = resolve(scriptDir, "../src-tauri/tauri.updater.conf.json");

const config = {
  bundle: {
    createUpdaterArtifacts: true,
  },
  plugins: {
    updater: {
      pubkey: publicKey,
      endpoints: [
        "https://github.com/diaryx-org/diaryx/releases/latest/download/latest.json",
      ],
      windows: {
        installMode: "passive",
      },
    },
  },
};

await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");

console.log(`Wrote updater config to ${outputPath}`);
