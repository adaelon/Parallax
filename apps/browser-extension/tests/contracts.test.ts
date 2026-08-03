import {
  MAX_PAGE_CONTENT_BYTES,
  MAX_QUEUE_BYTES,
  MAX_QUEUE_ITEMS,
  attachPageContent,
  enqueueBounded,
  finalizeVisit,
  normalizeQueue,
  permissionTarget,
  retainAuthorizedPageContent,
  startVisit,
  webTabSnapshot,
  type BrowserSubmissionPayload,
} from "../src/contracts";
import { describe, expect, it } from "vitest";

function submission(id: string, pageText: string | null = null): BrowserSubmissionPayload {
  return {
    submissionId: id,
    url: "https://example.test/article",
    title: "Article",
    visitedAtMillis: 1_000,
    dwellMillis: 1_000,
    pageContent:
      pageText === null
        ? null
        : {
            bodyText: pageText,
            capturedAtMillis: 1_500,
            authorizedOrigin: "https://example.test",
          },
  };
}

describe("browser visit metadata", () => {
  it("records one bounded URL/title visit and dwell interval", () => {
    const snapshot = webTabSnapshot({
      id: 7,
      windowId: 3,
      url: "https://example.test/article?q=1#part",
      title: "A title",
      incognito: false,
    });
    expect(snapshot).not.toBeNull();

    const visit = startVisit(snapshot!, 1_000, "visit-1");
    expect(finalizeVisit(visit, 1_750)).toEqual({
      submissionId: "visit-1",
      url: "https://example.test/article?q=1#part",
      title: "A title",
      visitedAtMillis: 1_000,
      dwellMillis: 750,
      pageContent: null,
    });
  });

  it("rejects private, credentialed, file, and browser-internal pages", () => {
    expect(
      webTabSnapshot({
        id: 1,
        windowId: 1,
        url: "https://example.test/",
        title: "Private",
        incognito: true,
      }),
    ).toBeNull();
    for (const url of [
      "file:///secret.txt",
      "chrome://history/",
      "https://person:secret@example.test/",
    ]) {
      expect(
        webTabSnapshot({ id: 1, windowId: 1, url, title: "Rejected", incognito: false }),
      ).toBeNull();
    }
  });
});

describe("source-authorized page text", () => {
  it("attaches bounded text to the exact source origin", () => {
    const visit = startVisit(
      {
        tabId: 1,
        windowId: 1,
        url: "https://example.test:8443/article",
        title: "Article",
      },
      1_000,
      "visit-content",
    );
    const attached = attachPageContent(visit, "正文", 1_500);

    expect(permissionTarget(visit.url)).toEqual({
      origin: "https://example.test:8443",
      pattern: "https://example.test:8443/*",
    });
    expect(attached.pageContent).toEqual({
      bodyText: "正文",
      capturedAtMillis: 1_500,
      authorizedOrigin: "https://example.test:8443",
    });
    expect(
      new TextEncoder().encode(attachPageContent(visit, "界".repeat(MAX_PAGE_CONTENT_BYTES), 1_500).pageContent!
        .bodyText).byteLength,
    ).toBeLessThanOrEqual(MAX_PAGE_CONTENT_BYTES);
  });

  it("strips queued text when its source authorization is revoked", () => {
    const queue = [submission("authorized", "kept"), submission("revoked", "removed")];
    queue[1].url = "https://revoked.test/page";
    queue[1].pageContent!.authorizedOrigin = "https://revoked.test";

    const filtered = retainAuthorizedPageContent(queue, new Set(["https://example.test"]));

    expect(filtered.stripped).toBe(1);
    expect(filtered.queue[0].pageContent?.bodyText).toBe("kept");
    expect(filtered.queue[1].pageContent).toBeNull();
  });
});

describe("bounded persistent retry queue", () => {
  it("deduplicates retry submissions by their stable identifier", () => {
    const first = enqueueBounded([], submission("same-id"));
    const retried = enqueueBounded(first.queue, submission("same-id"));

    expect(retried.queue).toHaveLength(1);
    expect(retried.dropped).toBe(0);
  });

  it("evicts oldest entries at both item and serialized-byte limits", () => {
    let queue: BrowserSubmissionPayload[] = [];
    let dropped = 0;
    for (let index = 0; index < MAX_QUEUE_ITEMS + 1; index += 1) {
      const result = enqueueBounded(queue, submission(`item-${index}`));
      queue = result.queue;
      dropped += result.dropped;
    }
    expect(queue).toHaveLength(MAX_QUEUE_ITEMS);
    expect(queue[0].submissionId).toBe("item-1");
    expect(dropped).toBe(1);

    queue = [];
    dropped = 0;
    const largeText = "x".repeat(MAX_PAGE_CONTENT_BYTES);
    for (let index = 0; index < 10; index += 1) {
      const result = enqueueBounded(queue, submission(`large-${index}`, largeText));
      queue = result.queue;
      dropped += result.dropped;
    }
    expect(new TextEncoder().encode(JSON.stringify(queue)).byteLength).toBeLessThanOrEqual(
      MAX_QUEUE_BYTES,
    );
    expect(dropped).toBeGreaterThan(0);
  });

  it("re-bounds persisted state and rejects malformed queue entries", () => {
    const restored = normalizeQueue([
      submission("valid"),
      { submissionId: "malformed" },
      submission("valid"),
    ]);

    expect(restored.queue.map((item) => item.submissionId)).toEqual(["valid"]);
    expect(restored.dropped).toBe(1);
  });
});
