import { type PermissionTarget, permissionTarget } from "./contracts";

const origin = requireElement<HTMLElement>("origin");
const status = requireElement<HTMLElement>("status");
const authorize = requireElement<HTMLButtonElement>("authorize");
const revoke = requireElement<HTMLButtonElement>("revoke");

let currentTarget: PermissionTarget | null = null;

async function loadCurrentTarget(): Promise<void> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  currentTarget = permissionTarget(tab?.url ?? "");
  await render();
}

async function render(message?: string): Promise<void> {
  if (currentTarget === null) {
    origin.textContent = "Unavailable";
    status.textContent = message ?? "Open an HTTP or HTTPS page to manage optional page text.";
    authorize.disabled = true;
    revoke.disabled = true;
    return;
  }
  const allowed = await chrome.permissions.contains({ origins: [currentTarget.pattern] });
  origin.textContent = currentTarget.origin;
  status.textContent =
    message ??
    (allowed
      ? "Page text is allowed for this source."
      : "Only browsing metadata is recorded for this source.");
  authorize.disabled = allowed;
  revoke.disabled = !allowed;
}

authorize.addEventListener("click", () => {
  const target = currentTarget;
  if (target === null) {
    return;
  }
  void chrome.permissions
    .request({ origins: [target.pattern] })
    .then(async (granted) => {
      if (granted) {
        await notifyAuthorizationChanged();
      }
      await render(granted ? undefined : "Page text permission was not granted.");
    })
    .catch(() => render("Page text permission could not be changed."));
});

revoke.addEventListener("click", () => {
  const target = currentTarget;
  if (target === null) {
    return;
  }
  void chrome.permissions
    .remove({ origins: [target.pattern] })
    .then(async (removed) => {
      if (removed) {
        await notifyAuthorizationChanged();
      }
      await render(removed ? undefined : "Page text permission was already absent.");
    })
    .catch(() => render("Page text permission could not be changed."));
});

async function notifyAuthorizationChanged(): Promise<void> {
  await chrome.runtime.sendMessage({ type: "refresh-active-content" });
}

function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing popup element: ${id}`);
  }
  return element as T;
}

void loadCurrentTarget().catch(() => render("Current source permission is unavailable."));
