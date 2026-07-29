// @vitest-environment jsdom

import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { App, ConversationTurn } from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  document.body.innerHTML = '<div id="root"></div>';
  root = createRoot(document.getElementById("root")!);
  invokeMock.mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
});

describe("S07 continuous conversation", () => {
  it("restores prior turns and appends a successful round through whitelisted commands", async () => {
    const restored = [turn(1, "person", "重启前的原话")];
    const result = {
      person: turn(2, "person", "接着聊吧"),
      counterpart: turn(3, "counterpart", "我还在这里。"),
    };
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return restored as T;
      }
      if (command === "send_message") {
        return result as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("重启前的原话"));

    await enterMessage("接着聊吧");
    await act(async () => {
      document.querySelector("form")!.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
    });

    await vi.waitFor(() => expect(document.body.textContent).toContain("我还在这里。"));
    expect(invokeMock).toHaveBeenCalledWith("send_message", {
      verbatim: "接着聊吧",
    });
    expect(document.body.textContent).toContain("重启前的原话");
  });

  it("reloads an already-persisted person turn when the runtime response fails", async () => {
    const persisted = [turn(4, "person", "即使失败也保留这句")];
    let listCalls = 0;
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        listCalls += 1;
        return (listCalls === 1 ? [] : persisted) as T;
      }
      if (command === "send_message") {
        throw new Error("模型运行时暂不可用");
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));

    await enterMessage("即使失败也保留这句");
    await act(async () => {
      document.querySelector("form")!.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
    });

    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("模型运行时暂不可用");
      expect(document.body.textContent).toContain("即使失败也保留这句");
    });
    expect(listCalls).toBe(2);
  });
});

async function enterMessage(message: string) {
  const textarea = document.querySelector("textarea")!;
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )!.set!;
  await act(async () => {
    setter.call(textarea, message);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function turn(
  id: number,
  speaker: ConversationTurn["speaker"],
  verbatim: string,
): ConversationTurn {
  return { id, speaker, verbatim, recordedAtMillis: 1_785_000_000_000 + id };
}
