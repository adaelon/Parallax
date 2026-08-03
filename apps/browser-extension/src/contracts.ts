export const HOST_BASE_URL = "http://127.0.0.1:43129";
export const PROTOCOL_VERSION = "eam-browser-capture-v1";
export const EXPECTED_EXTENSION_ID = "knpliheabhbegfjbgdiaclndgnjelggh";
export const MAX_URL_BYTES = 16 * 1024;
export const MAX_TITLE_BYTES = 16 * 1024;
export const MAX_PAGE_CONTENT_BYTES = 512 * 1024;
export const MAX_DWELL_MILLIS = 24 * 60 * 60 * 1000;
export const MAX_SUBMISSION_ID_BYTES = 64;
export const MAX_QUEUE_ITEMS = 128;
export const MAX_QUEUE_BYTES = 4 * 1024 * 1024;

export interface PageContentPayload {
  bodyText: string;
  capturedAtMillis: number;
  authorizedOrigin: string;
}

export interface BrowserSubmissionPayload {
  submissionId: string;
  url: string;
  title: string;
  visitedAtMillis: number;
  dwellMillis: number;
  pageContent: PageContentPayload | null;
}

export interface ActiveVisit {
  submissionId: string;
  tabId: number;
  windowId: number;
  url: string;
  title: string;
  visitedAtMillis: number;
  pageContent: PageContentPayload | null;
}

export interface WebTabSnapshot {
  tabId: number;
  windowId: number;
  url: string;
  title: string;
}

export interface PermissionTarget {
  origin: string;
  pattern: string;
}

export interface QueueResult {
  queue: BrowserSubmissionPayload[];
  dropped: number;
}

export interface AuthorizationFilterResult {
  queue: BrowserSubmissionPayload[];
  stripped: number;
}

export type SubmissionDisposition = "accepted" | "drop" | "retry";

const encoder = new TextEncoder();

export function webTabSnapshot(
  tab: Pick<chrome.tabs.Tab, "id" | "windowId" | "url" | "title" | "incognito">,
): WebTabSnapshot | null {
  if (tab.incognito || tab.id === undefined || tab.url === undefined) {
    return null;
  }
  const url = canonicalWebUrl(tab.url);
  if (url === null) {
    return null;
  }
  return {
    tabId: tab.id,
    windowId: tab.windowId,
    url,
    title: truncateUtf8(tab.title ?? "", MAX_TITLE_BYTES),
  };
}

export function startVisit(
  tab: WebTabSnapshot,
  visitedAtMillis: number,
  submissionId: string,
): ActiveVisit {
  return {
    submissionId,
    tabId: tab.tabId,
    windowId: tab.windowId,
    url: tab.url,
    title: truncateUtf8(tab.title, MAX_TITLE_BYTES),
    visitedAtMillis: normalizeMillis(visitedAtMillis),
    pageContent: null,
  };
}

export function finalizeVisit(
  visit: ActiveVisit,
  endedAtMillis: number,
): BrowserSubmissionPayload {
  const dwellMillis = Math.min(
    MAX_DWELL_MILLIS,
    Math.max(0, normalizeMillis(endedAtMillis) - visit.visitedAtMillis),
  );
  const acceptedThrough = visit.visitedAtMillis + dwellMillis;
  const pageContent =
    visit.pageContent !== null && visit.pageContent.capturedAtMillis <= acceptedThrough
      ? visit.pageContent
      : null;
  return {
    submissionId: visit.submissionId,
    url: visit.url,
    title: truncateUtf8(visit.title, MAX_TITLE_BYTES),
    visitedAtMillis: visit.visitedAtMillis,
    dwellMillis,
    pageContent,
  };
}

export function attachPageContent(
  visit: ActiveVisit,
  bodyText: string,
  capturedAtMillis: number,
): ActiveVisit {
  const target = permissionTarget(visit.url);
  const bounded = truncateUtf8(bodyText, MAX_PAGE_CONTENT_BYTES);
  if (target === null || bounded.length === 0) {
    return { ...visit, pageContent: null };
  }
  return {
    ...visit,
    pageContent: {
      bodyText: bounded,
      capturedAtMillis: Math.max(visit.visitedAtMillis, normalizeMillis(capturedAtMillis)),
      authorizedOrigin: target.origin,
    },
  };
}

export function permissionTarget(rawUrl: string): PermissionTarget | null {
  const canonical = canonicalWebUrl(rawUrl);
  if (canonical === null) {
    return null;
  }
  const url = new URL(canonical);
  return {
    origin: url.origin,
    pattern: `${url.protocol}//${url.host}/*`,
  };
}

export function enqueueBounded(
  queue: readonly BrowserSubmissionPayload[],
  submission: BrowserSubmissionPayload,
): QueueResult {
  if (!isBrowserSubmissionPayload(submission)) {
    return { queue: [...queue], dropped: 1 };
  }
  if (queue.some((queued) => queued.submissionId === submission.submissionId)) {
    return { queue: [...queue], dropped: 0 };
  }
  if (jsonBytes(submission) > MAX_QUEUE_BYTES) {
    return { queue: [...queue], dropped: 1 };
  }
  const next = [...queue, submission];
  let dropped = 0;
  while (next.length > MAX_QUEUE_ITEMS || jsonBytes(next) > MAX_QUEUE_BYTES) {
    next.shift();
    dropped += 1;
  }
  return { queue: next, dropped };
}

export function normalizeQueue(value: unknown): QueueResult {
  if (!Array.isArray(value)) {
    return { queue: [], dropped: 0 };
  }
  let queue: BrowserSubmissionPayload[] = [];
  let dropped = 0;
  for (const candidate of value) {
    if (!isBrowserSubmissionPayload(candidate)) {
      dropped += 1;
      continue;
    }
    const result = enqueueBounded(queue, candidate);
    queue = result.queue;
    dropped += result.dropped;
  }
  return { queue, dropped };
}

export function retainAuthorizedPageContent(
  queue: readonly BrowserSubmissionPayload[],
  authorizedOrigins: ReadonlySet<string>,
): AuthorizationFilterResult {
  let stripped = 0;
  const filtered = queue.map((submission) => {
    const content = submission.pageContent;
    if (content === null || authorizedOrigins.has(content.authorizedOrigin)) {
      return submission;
    }
    stripped += 1;
    return { ...submission, pageContent: null };
  });
  return { queue: filtered, stripped };
}

export function isBrowserSubmissionPayload(value: unknown): value is BrowserSubmissionPayload {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<BrowserSubmissionPayload>;
  if (
    typeof candidate.submissionId !== "string" ||
    encoder.encode(candidate.submissionId).byteLength > MAX_SUBMISSION_ID_BYTES ||
    !/^[A-Za-z0-9_-]+$/.test(candidate.submissionId) ||
    typeof candidate.url !== "string" ||
    canonicalWebUrl(candidate.url) === null ||
    typeof candidate.title !== "string" ||
    encoder.encode(candidate.title).byteLength > MAX_TITLE_BYTES ||
    !isSafeMillis(candidate.visitedAtMillis) ||
    !isSafeMillis(candidate.dwellMillis) ||
    candidate.dwellMillis > MAX_DWELL_MILLIS ||
    candidate.visitedAtMillis + candidate.dwellMillis > Number.MAX_SAFE_INTEGER
  ) {
    return false;
  }
  if (candidate.pageContent === null) {
    return true;
  }
  const content = candidate.pageContent as Partial<PageContentPayload> | undefined;
  const target = permissionTarget(candidate.url);
  return (
    typeof content === "object" &&
    content !== null &&
    typeof content.bodyText === "string" &&
    content.bodyText.length > 0 &&
    encoder.encode(content.bodyText).byteLength <= MAX_PAGE_CONTENT_BYTES &&
    isSafeMillis(content.capturedAtMillis) &&
    content.capturedAtMillis >= candidate.visitedAtMillis &&
    content.capturedAtMillis <= candidate.visitedAtMillis + candidate.dwellMillis &&
    typeof content.authorizedOrigin === "string" &&
    target !== null &&
    content.authorizedOrigin === target.origin
  );
}

export function submissionDisposition(status: number): SubmissionDisposition {
  if (status === 202) {
    return "accepted";
  }
  if ([400, 413, 415, 422].includes(status)) {
    return "drop";
  }
  return "retry";
}

export function truncateUtf8(value: string, maximumBytes: number): string {
  if (encoder.encode(value).byteLength <= maximumBytes) {
    return value;
  }
  let result = "";
  let bytes = 0;
  for (const character of value) {
    const encoded = encoder.encode(character).byteLength;
    if (bytes + encoded > maximumBytes) {
      break;
    }
    result += character;
    bytes += encoded;
  }
  return result;
}

function canonicalWebUrl(rawUrl: string): string | null {
  if (encoder.encode(rawUrl).byteLength > MAX_URL_BYTES) {
    return null;
  }
  try {
    const url = new URL(rawUrl);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.hostname.length === 0 ||
      url.username.length !== 0 ||
      url.password.length !== 0
    ) {
      return null;
    }
    return url.toString();
  } catch {
    return null;
  }
}

function normalizeMillis(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.trunc(value));
}

function isSafeMillis(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function jsonBytes(value: unknown): number {
  return encoder.encode(JSON.stringify(value)).byteLength;
}
