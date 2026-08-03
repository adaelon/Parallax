import {
  HOST_BASE_URL,
  MAX_DWELL_MILLIS,
  MAX_PAGE_CONTENT_BYTES,
  PROTOCOL_VERSION,
  type ActiveVisit,
  type BrowserSubmissionPayload,
  attachPageContent,
  enqueueBounded,
  finalizeVisit,
  normalizeQueue,
  permissionTarget,
  retainAuthorizedPageContent,
  startVisit,
  submissionDisposition,
  webTabSnapshot,
} from "./contracts";

const RUNTIME_STORAGE_KEY = "captureRuntimeV1";
const QUEUE_STORAGE_KEY = "captureQueueV1";
const RETRY_ALARM = "browser-capture-retry";
const RETRY_PERIOD_MINUTES = 0.5;

interface RuntimeState {
  activeVisit: ActiveVisit | null;
  focusedWindowId: number | null;
}

interface PersistentQueueState {
  queue: BrowserSubmissionPayload[];
  droppedCount: number;
}

let sessionToken: string | null = null;
let work: Promise<void> = Promise.resolve();

function exclusive<T>(task: () => Promise<T>): Promise<T> {
  const result = work.then(task, task);
  work = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}

function schedule(task: () => Promise<void>): void {
  void exclusive(task).catch(() => undefined);
}

async function loadRuntimeState(): Promise<RuntimeState> {
  const stored = await chrome.storage.session.get(RUNTIME_STORAGE_KEY);
  const candidate = stored[RUNTIME_STORAGE_KEY] as Partial<RuntimeState> | undefined;
  return {
    activeVisit: candidate?.activeVisit ?? null,
    focusedWindowId:
      typeof candidate?.focusedWindowId === "number" ? candidate.focusedWindowId : null,
  };
}

async function saveRuntimeState(state: RuntimeState): Promise<void> {
  await chrome.storage.session.set({ [RUNTIME_STORAGE_KEY]: state });
}

async function loadQueueState(): Promise<PersistentQueueState> {
  const stored = await chrome.storage.local.get(QUEUE_STORAGE_KEY);
  const candidate = stored[QUEUE_STORAGE_KEY] as Partial<PersistentQueueState> | undefined;
  const normalized = normalizeQueue(candidate?.queue);
  const droppedCount =
    (typeof candidate?.droppedCount === "number"
      ? Math.max(0, Math.trunc(candidate.droppedCount))
      : 0) + normalized.dropped;
  const state = { queue: normalized.queue, droppedCount };
  if (normalized.dropped > 0) {
    await saveQueueState(state);
  }
  return state;
}

async function saveQueueState(state: PersistentQueueState): Promise<void> {
  await chrome.storage.local.set({ [QUEUE_STORAGE_KEY]: state });
}

async function ensureRetryAlarm(): Promise<void> {
  if ((await chrome.alarms.get(RETRY_ALARM)) === undefined) {
    await chrome.alarms.create(RETRY_ALARM, { periodInMinutes: RETRY_PERIOD_MINUTES });
  }
}

async function initialize(): Promise<void> {
  await chrome.storage.local.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
  await chrome.storage.session.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
  await ensureRetryAlarm();
  await reconcileStoredAuthorizations();
  await reconcileFocusedTab(Date.now());
  await flushQueue();
}

async function reconcileFocusedTab(atMillis: number): Promise<void> {
  const focusedWindow = await chrome.windows.getLastFocused();
  if (!focusedWindow.focused || focusedWindow.id === undefined) {
    let runtime = await loadRuntimeState();
    runtime = await closeActiveVisit(runtime, atMillis);
    runtime.focusedWindowId = null;
    await saveRuntimeState(runtime);
    return;
  }
  const [tab] = await chrome.tabs.query({ active: true, windowId: focusedWindow.id });
  if (tab === undefined) {
    return;
  }
  await transitionToTab(tab, focusedWindow.id, atMillis, tab.status === "complete");
}

async function transitionToTab(
  tab: chrome.tabs.Tab,
  focusedWindowId: number,
  atMillis: number,
  captureContent: boolean,
): Promise<void> {
  let runtime = await loadRuntimeState();
  runtime.focusedWindowId = focusedWindowId;
  const snapshot = webTabSnapshot(tab);
  const current = runtime.activeVisit;
  if (
    current !== null &&
    snapshot !== null &&
    current.tabId === snapshot.tabId &&
    current.url === snapshot.url
  ) {
    runtime.activeVisit = { ...current, title: snapshot.title };
    if (captureContent) {
      runtime = await refreshActivePageContent(runtime, atMillis);
    }
    await saveRuntimeState(runtime);
    await flushQueue();
    return;
  }

  runtime = await closeActiveVisit(runtime, atMillis);
  if (snapshot !== null && snapshot.windowId === focusedWindowId) {
    runtime.activeVisit = startVisit(snapshot, atMillis, crypto.randomUUID());
    if (captureContent) {
      runtime = await refreshActivePageContent(runtime, atMillis);
    }
  }
  await saveRuntimeState(runtime);
  await flushQueue();
}

async function closeActiveVisit(runtime: RuntimeState, atMillis: number): Promise<RuntimeState> {
  if (runtime.activeVisit === null) {
    return runtime;
  }
  runtime = await refreshActivePageContent(runtime, atMillis);
  if (runtime.activeVisit === null) {
    return runtime;
  }
  const submission = finalizeVisit(runtime.activeVisit, atMillis);
  const persistent = await loadQueueState();
  const queued = enqueueBounded(persistent.queue, submission);
  await saveQueueState({
    queue: queued.queue,
    droppedCount: persistent.droppedCount + queued.dropped,
  });
  return { ...runtime, activeVisit: null };
}

async function refreshActivePageContent(
  runtime: RuntimeState,
  capturedAtMillis: number,
): Promise<RuntimeState> {
  const active = runtime.activeVisit;
  if (active === null) {
    return runtime;
  }
  const permission = permissionTarget(active.url);
  if (
    permission === null ||
    !(await chrome.permissions.contains({ origins: [permission.pattern] }))
  ) {
    return { ...runtime, activeVisit: { ...active, pageContent: null } };
  }
  try {
    const tab = await chrome.tabs.get(active.tabId);
    const snapshot = webTabSnapshot(tab);
    if (snapshot === null || snapshot.url !== active.url) {
      return runtime;
    }
    const [injection] = await chrome.scripting.executeScript({
      target: { tabId: active.tabId },
      func: extractBoundedPageText,
    });
    const bodyText = typeof injection?.result === "string" ? injection.result : "";
    return {
      ...runtime,
      activeVisit: attachPageContent(active, bodyText, capturedAtMillis),
    };
  } catch {
    return runtime;
  }
}

function extractBoundedPageText(): string {
  const text = document.body?.innerText ?? "";
  const encoder = new TextEncoder();
  if (encoder.encode(text).byteLength <= MAX_PAGE_CONTENT_BYTES) {
    return text;
  }
  let result = "";
  let bytes = 0;
  for (const character of text) {
    const encoded = encoder.encode(character).byteLength;
    if (bytes + encoded > MAX_PAGE_CONTENT_BYTES) {
      break;
    }
    result += character;
    bytes += encoded;
  }
  return result;
}

async function reconcileStoredAuthorizations(): Promise<void> {
  const persistent = await loadQueueState();
  const origins = new Set(
    persistent.queue.flatMap((submission) =>
      submission.pageContent === null ? [] : [submission.pageContent.authorizedOrigin],
    ),
  );
  const authorizedOrigins = new Set<string>();
  for (const origin of origins) {
    const target = permissionTarget(origin);
    if (
      target !== null &&
      (await chrome.permissions.contains({ origins: [target.pattern] }))
    ) {
      authorizedOrigins.add(origin);
    }
  }
  const filtered = retainAuthorizedPageContent(persistent.queue, authorizedOrigins);
  if (filtered.stripped > 0) {
    await saveQueueState({ ...persistent, queue: filtered.queue });
  }

  let runtime = await loadRuntimeState();
  runtime = await refreshActivePageContent(runtime, Date.now());
  await saveRuntimeState(runtime);
}

async function rotateOverlongVisit(atMillis: number): Promise<void> {
  let runtime = await loadRuntimeState();
  const active = runtime.activeVisit;
  if (active === null || atMillis - active.visitedAtMillis < MAX_DWELL_MILLIS) {
    await flushQueue();
    return;
  }
  runtime = await closeActiveVisit(runtime, active.visitedAtMillis + MAX_DWELL_MILLIS);
  await saveRuntimeState(runtime);
  await reconcileFocusedTab(atMillis);
  await flushQueue();
}

async function flushQueue(): Promise<void> {
  const persistent = await loadQueueState();
  let droppedCount = persistent.droppedCount;
  let changed = false;
  while (persistent.queue.length > 0) {
    let disposition: ReturnType<typeof submissionDisposition>;
    try {
      disposition = await submit(persistent.queue[0]);
    } catch {
      break;
    }
    if (disposition === "retry") {
      break;
    }
    persistent.queue.shift();
    changed = true;
    if (disposition === "drop") {
      droppedCount += 1;
    }
  }
  if (changed || droppedCount !== persistent.droppedCount) {
    await saveQueueState({ ...persistent, droppedCount });
  }
}

async function submit(
  submission: BrowserSubmissionPayload,
  refreshToken = false,
): Promise<ReturnType<typeof submissionDisposition>> {
  const token = await getSessionToken(refreshToken);
  const response = await fetch(`${HOST_BASE_URL}/v1/browser-events`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(submission),
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    referrerPolicy: "no-referrer",
    signal: AbortSignal.timeout(2_000),
  });
  if (response.status === 401 && !refreshToken) {
    sessionToken = null;
    return submit(submission, true);
  }
  return submissionDisposition(response.status);
}

async function getSessionToken(forceRefresh: boolean): Promise<string> {
  if (!forceRefresh && sessionToken !== null) {
    return sessionToken;
  }
  const response = await fetch(`${HOST_BASE_URL}/v1/session`, {
    method: "GET",
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    referrerPolicy: "no-referrer",
    signal: AbortSignal.timeout(2_000),
  });
  if (!response.ok) {
    throw new Error("local browser capture session is unavailable");
  }
  const payload = (await response.json()) as { protocolVersion?: unknown; token?: unknown };
  if (
    payload.protocolVersion !== PROTOCOL_VERSION ||
    typeof payload.token !== "string" ||
    !/^[0-9a-f]{64}$/.test(payload.token)
  ) {
    throw new Error("local browser capture session is invalid");
  }
  sessionToken = payload.token;
  return sessionToken;
}

chrome.runtime.onInstalled.addListener(() => schedule(initialize));
chrome.runtime.onStartup.addListener(() => schedule(initialize));
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === RETRY_ALARM) {
    schedule(() => rotateOverlongVisit(Date.now()));
  }
});
chrome.windows.onFocusChanged.addListener((windowId) => {
  schedule(async () => {
    if (windowId === chrome.windows.WINDOW_ID_NONE) {
      let runtime = await loadRuntimeState();
      runtime = await closeActiveVisit(runtime, Date.now());
      runtime.focusedWindowId = null;
      await saveRuntimeState(runtime);
      await flushQueue();
      return;
    }
    const [tab] = await chrome.tabs.query({ active: true, windowId });
    if (tab !== undefined) {
      await transitionToTab(tab, windowId, Date.now(), tab.status === "complete");
    }
  });
});
chrome.tabs.onActivated.addListener((activeInfo) => {
  schedule(async () => {
    const runtime = await loadRuntimeState();
    if (runtime.focusedWindowId !== activeInfo.windowId) {
      return;
    }
    const tab = await chrome.tabs.get(activeInfo.tabId);
    await transitionToTab(tab, activeInfo.windowId, Date.now(), tab.status === "complete");
  });
});
chrome.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  schedule(async () => {
    const runtime = await loadRuntimeState();
    const isCurrent = runtime.activeVisit?.tabId === tab.id;
    if (runtime.focusedWindowId !== tab.windowId || (!tab.active && !isCurrent)) {
      return;
    }
    if (changeInfo.url !== undefined || tab.active) {
      await transitionToTab(tab, tab.windowId, Date.now(), changeInfo.status === "complete");
    }
  });
});
chrome.tabs.onRemoved.addListener((tabId) => {
  schedule(async () => {
    let runtime = await loadRuntimeState();
    if (runtime.activeVisit?.tabId !== tabId) {
      return;
    }
    runtime = await closeActiveVisit(runtime, Date.now());
    await saveRuntimeState(runtime);
    await flushQueue();
  });
});
chrome.permissions.onRemoved.addListener(() => schedule(reconcileStoredAuthorizations));
chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  if (
    typeof message !== "object" ||
    message === null ||
    !("type" in message) ||
    message.type !== "refresh-active-content"
  ) {
    return false;
  }
  void exclusive(reconcileStoredAuthorizations).then(
    () => sendResponse({ ok: true }),
    () => sendResponse({ ok: false }),
  );
  return true;
});

schedule(initialize);
