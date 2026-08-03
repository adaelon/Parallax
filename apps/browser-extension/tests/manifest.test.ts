import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { EXPECTED_EXTENSION_ID, HOST_BASE_URL } from "../src/contracts";

interface ManifestV3 {
  manifest_version: number;
  key: string;
  permissions: string[];
  host_permissions: string[];
  optional_host_permissions: string[];
  background: { service_worker: string; type: string };
  action: { default_popup: string };
}

const extensionRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(extensionRoot, "..", "..");

function manifest(): ManifestV3 {
  return JSON.parse(
    readFileSync(resolve(extensionRoot, "public", "manifest.json"), "utf8"),
  ) as ManifestV3;
}

describe("Manifest V3 permission and origin audit", () => {
  it("declares exactly the API and host capabilities exercised by the extension", () => {
    const source = ["src/contracts.ts", "src/service-worker.ts", "src/popup.ts"]
      .map((path) => readFileSync(resolve(extensionRoot, path), "utf8"))
      .join("\n");
    const namespaces = new Set(
      [...source.matchAll(/\bchrome\.([A-Za-z][A-Za-z0-9]*)/g)].map((match) => match[1]),
    );
    expect([...namespaces].sort()).toEqual([
      "alarms",
      "permissions",
      "runtime",
      "scripting",
      "storage",
      "tabs",
      "windows",
    ]);

    const requiredNamedPermissions = [...namespaces]
      .filter((namespace) => ["alarms", "scripting", "storage", "tabs"].includes(namespace))
      .sort();
    const parsed = manifest();
    expect(parsed.manifest_version).toBe(3);
    expect([...parsed.permissions].sort()).toEqual(requiredNamedPermissions);
    expect(parsed.host_permissions).toEqual([`${HOST_BASE_URL}/*`]);
    expect(parsed.optional_host_permissions).toEqual(["http://*/*", "https://*/*"]);
    expect(parsed.background).toEqual({ service_worker: "service-worker.js", type: "module" });
    expect(parsed.action.default_popup).toBe("popup.html");
  });

  it("derives the same fixed extension ID used by the loopback origin gate", () => {
    const key = Buffer.from(manifest().key, "base64");
    const idHex = createHash("sha256").update(key).digest("hex").slice(0, 32);
    const derivedId = [...idHex]
      .map((digit) => String.fromCharCode("a".charCodeAt(0) + Number.parseInt(digit, 16)))
      .join("");
    const rustHttp = readFileSync(
      resolve(repositoryRoot, "crates", "capture-browser", "src", "http.rs"),
      "utf8",
    );

    expect(derivedId).toBe(EXPECTED_EXTENSION_ID);
    expect(rustHttp).toContain(`pub const EXTENSION_ID: &str = "${EXPECTED_EXTENSION_ID}";`);
    expect(rustHttp).toContain(
      `pub const EXTENSION_ORIGIN: &str = "chrome-extension://${EXPECTED_EXTENSION_ID}";`,
    );
  });

  it("keeps every declared production entry local to the extension package", () => {
    expect(existsSync(resolve(extensionRoot, "src", "service-worker.ts"))).toBe(true);
    expect(existsSync(resolve(extensionRoot, "src", "popup.ts"))).toBe(true);
    const worker = readFileSync(resolve(extensionRoot, "src", "service-worker.ts"), "utf8");
    expect(worker).toContain("chrome.storage.local.get(QUEUE_STORAGE_KEY)");
    expect(worker).toContain("chrome.storage.local.set({ [QUEUE_STORAGE_KEY]: state })");
    expect(worker).toContain("chrome.storage.session.get(RUNTIME_STORAGE_KEY)");
    expect(worker).toContain("chrome.storage.session.set({ [RUNTIME_STORAGE_KEY]: state })");
    const popup = readFileSync(resolve(extensionRoot, "popup.html"), "utf8");
    expect(popup).not.toMatch(/https?:\/\//);
    expect(popup).toContain('src="/src/popup.ts"');
  });
});
