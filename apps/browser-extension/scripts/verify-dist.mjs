import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const dist = resolve(root, "dist");
const manifestPath = resolve(dist, "manifest.json");

if (!existsSync(manifestPath)) {
  throw new Error("production extension is missing dist/manifest.json");
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const required = [manifest.background?.service_worker, manifest.action?.default_popup];
for (const relativePath of required) {
  if (typeof relativePath !== "string" || !existsSync(resolve(dist, relativePath))) {
    throw new Error(`production extension entry is missing: ${String(relativePath)}`);
  }
}

const popupPath = resolve(dist, manifest.action.default_popup);
const popup = readFileSync(popupPath, "utf8");
if (popup.includes("/src/")) {
  throw new Error("production popup still references source files");
}
for (const match of popup.matchAll(/(?:src|href)="([^"]+)"/g)) {
  const reference = match[1];
  if (reference.startsWith("data:") || reference.startsWith("#")) {
    continue;
  }
  const localPath = reference.startsWith("/") ? reference.slice(1) : reference;
  if (!existsSync(resolve(dist, localPath))) {
    throw new Error(`production popup asset is missing: ${reference}`);
  }
}

const worker = readFileSync(resolve(dist, manifest.background.service_worker), "utf8");
if (worker.length === 0 || /https?:\/\/(?!127\.0\.0\.1:43129)/.test(worker)) {
  throw new Error("production service worker is empty or contains an undeclared remote endpoint");
}

process.stdout.write("verified loadable Manifest V3 production directory\n");
