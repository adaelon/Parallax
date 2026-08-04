// @vitest-environment jsdom

import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ActivityTimelineEntry,
  App,
  ConversationTurn,
  IdentityStateVersion,
  ReflectionInvitationCeremony,
  SharedExperienceCeremony,
} from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
let root: Root;

const READY_HOST_STATUS = {
  state: "foregroundRunning",
  vaultReady: true,
  updaterConfigured: false,
  detail: null,
};
const RUNTIME_REPLACEMENT_KEY = "synthetic-runtime-secret-4321";

type InvokeHandler = <T>(command: string) => Promise<T>;

function mockReadyInvoke(implementation: InvokeHandler) {
  invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
    if (command === "get_host_status") {
      return READY_HOST_STATUS as T;
    }
    return implementation<T>(command);
  });
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  document.body.innerHTML = '<div id="root"></div>';
  root = createRoot(document.getElementById("root")!);
  invokeMock.mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
});

describe("first-run encrypted vault setup", () => {
  it("shows the recovery key once and loads trusted data only after confirmation", async () => {
    const recoveryKey = "eamrecovery1first-run-test-carrier";
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
      if (command === "get_host_status") {
        return {
          state: "needsInitialization",
          vaultReady: false,
          updaterConfigured: false,
          detail: null,
        } as T;
      }
      if (command === "initialize_vault") {
        return { recoveryKey } as T;
      }
      if (command === "confirm_recovery_key_saved") {
        return READY_HOST_STATUS as T;
      }
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies" ||
        command === "list_identity_history" ||
        command === "list_offered_reflection_invitations" ||
        command === "list_activity_timeline"
      ) {
        return [] as T;
      }
      if (command === "get_capture_status") {
        return { state: "collecting" } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("创建你的加密保险库"),
    );
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      "get_host_status",
    ]);

    await clickButton("生成恢复密钥");
    await vi.waitFor(() => expect(document.body.textContent).toContain(recoveryKey));
    const confirmButton = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === "我已安全保存，创建保险库",
    ) as HTMLButtonElement;
    expect(confirmButton.disabled).toBe(true);

    await clickButton("复制恢复密钥");
    expect(writeText).toHaveBeenCalledWith(recoveryKey);
    const checkbox = document.querySelector<HTMLInputElement>("#recovery-key-saved")!;
    await act(async () => checkbox.click());
    expect(confirmButton.disabled).toBe(false);
    await clickButton("我已安全保存，创建保险库");

    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("从此刻继续认识彼此"),
    );
    expect(document.body.textContent).not.toContain(recoveryKey);
    expect(invokeMock).toHaveBeenCalledWith("confirm_recovery_key_saved", {
      confirmed: true,
    });
  });
});

describe("S07 continuous conversation", () => {
  it("restores prior turns and appends a successful round through whitelisted commands", async () => {
    const restored = [turn(1, "person", "重启前的原话")];
    const result = {
      person: turn(2, "person", "接着聊吧"),
      counterpart: turn(3, "counterpart", "我还在这里。"),
      ceremonies: [],
    };
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return restored as T;
      }
      if (command === "send_message") {
        return result as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [] as T;
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
    let ceremonyCalls = 0;
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        listCalls += 1;
        return (listCalls === 1 ? [] : persisted) as T;
      }
      if (command === "send_message") {
        throw new Error("模型运行时暂不可用");
      }
      if (command === "list_shared_experience_ceremonies") {
        ceremonyCalls += 1;
        return (ceremonyCalls === 1 ? [] : [agreementCeremony()]) as T;
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
      expect(document.body.textContent).toContain("共同约定待确认");
    });
    expect(listCalls).toBe(2);
    expect(ceremonyCalls).toBe(2);
  });
});

describe("S06R-4 local runtime settings", () => {
  it("loads only the redacted profile and starts with an empty password field", async () => {
    const completeKeyThatMustNotRender = "complete-read-secret-2468";
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies"
      ) {
        return [] as T;
      }
      if (command === "get_runtime_profile") {
        return {
          ...runtimeProfileView(),
          apiKeyLastFour: "2468",
          apiKey: completeKeyThatMustNotRender,
        } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("运行时设置");

    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("已配置，末四位 2468"),
    );
    expect(document.querySelector<HTMLInputElement>("#runtime-base-url")!.value).toBe(
      "http://127.0.0.1:11434/v1",
    );
    expect(document.querySelector<HTMLInputElement>("#runtime-model")!.value).toBe(
      "gpt-oss-20b",
    );
    const password = document.querySelector<HTMLInputElement>("#runtime-api-key")!;
    expect(password.type).toBe("password");
    expect(password.value).toBe("");
    expect(document.body.textContent).not.toContain(completeKeyThatMustNotRender);
    expect(document.body.textContent).toContain(
      "官方 DeepSeek 地址会自动使用 Chat Completions",
    );
    expect(document.body.textContent).toContain("/chat/completions");
    expect(document.activeElement).toBe(
      document.querySelector("#runtime-base-url"),
    );
  });

  it("maps blank, replacement, and confirmed clearing to KEEP, REPLACE, and CLEAR without saving", async () => {
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies"
      ) {
        return [] as T;
      }
      if (command === "get_runtime_profile") {
        return runtimeProfileView() as T;
      }
      if (command === "test_runtime_profile") {
        return { succeeded: true } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("运行时设置");
    await vi.waitFor(() => expect(document.querySelector("#runtime-api-key")).not.toBeNull());

    await clickButton("测试连接");
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("草稿尚未保存"),
    );

    await setControlValue("#runtime-api-key", RUNTIME_REPLACEMENT_KEY);
    await clickButton("测试连接");
    await vi.waitFor(() =>
      expect(
        document.querySelector<HTMLInputElement>("#runtime-api-key")!.value,
      ).toBe(RUNTIME_REPLACEMENT_KEY),
    );

    const clear = document.querySelector<HTMLInputElement>("#runtime-clear-key")!;
    await act(async () => clear.click());
    expect(document.querySelector<HTMLInputElement>("#runtime-api-key")!.value).toBe("");
    await clickButton("测试连接");

    const testDrafts = invokeMock.mock.calls
      .filter(([command]) => command === "test_runtime_profile")
      .map(([, args]) => args);
    expect(testDrafts).toEqual([
      {
        draft: {
          baseUrl: "http://127.0.0.1:11434/v1",
          model: "gpt-oss-20b",
          apiKeyChange: { action: "KEEP" },
        },
      },
      {
        draft: {
          baseUrl: "http://127.0.0.1:11434/v1",
          model: "gpt-oss-20b",
          apiKeyChange: {
            action: "REPLACE",
            value: RUNTIME_REPLACEMENT_KEY,
          },
        },
      },
      {
        draft: {
          baseUrl: "http://127.0.0.1:11434/v1",
          model: "gpt-oss-20b",
          apiKeyChange: { action: "CLEAR" },
        },
      },
    ]);
    expect(
      invokeMock.mock.calls.some(([command]) => command === "save_runtime_profile"),
    ).toBe(false);
  });

  it("clears the replacement input and refreshes the redacted view after saving", async () => {
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies"
      ) {
        return [] as T;
      }
      if (command === "get_runtime_profile") {
        return runtimeProfileView() as T;
      }
      if (command === "save_runtime_profile") {
        return runtimeProfileView({
          baseUrl: "https://runtime.example.test/v1",
          model: "new-model",
          apiKeyLastFour: "4321",
        }) as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("运行时设置");
    await vi.waitFor(() => expect(document.querySelector("#runtime-base-url")).not.toBeNull());
    await setControlValue("#runtime-base-url", "https://runtime.example.test/v1");
    await setControlValue("#runtime-model", "new-model");
    await setControlValue("#runtime-api-key", RUNTIME_REPLACEMENT_KEY);
    await clickButton("保存并切换");

    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("运行时档案已保存并切换");
      expect(document.body.textContent).toContain("已配置，末四位 4321");
      expect(document.querySelector<HTMLInputElement>("#runtime-api-key")!.value).toBe("");
    });
    expect(invokeMock).toHaveBeenCalledWith("save_runtime_profile", {
      draft: {
        baseUrl: "https://runtime.example.test/v1",
        model: "new-model",
        apiKeyChange: {
          action: "REPLACE",
          value: RUNTIME_REPLACEMENT_KEY,
        },
      },
    });
    expect(document.body.textContent).not.toContain(RUNTIME_REPLACEMENT_KEY);
  });

  it("keeps the draft and shows a fixed redacted error when saving fails", async () => {
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies"
      ) {
        return [] as T;
      }
      if (command === "get_runtime_profile") {
        return runtimeProfileView() as T;
      }
      if (command === "save_runtime_profile") {
        throw new Error(`provider echoed ${RUNTIME_REPLACEMENT_KEY}`);
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("运行时设置");
    await vi.waitFor(() => expect(document.querySelector("#runtime-base-url")).not.toBeNull());
    await setControlValue("#runtime-base-url", "https://draft.example.test/v1");
    await setControlValue("#runtime-model", "draft-model");
    await setControlValue("#runtime-api-key", RUNTIME_REPLACEMENT_KEY);
    await clickButton("保存并切换");

    await vi.waitFor(() =>
      expect(document.body.textContent).toContain(
        "保存并切换失败；草稿和当前运行时均保持不变。",
      ),
    );
    expect(document.querySelector<HTMLInputElement>("#runtime-base-url")!.value).toBe(
      "https://draft.example.test/v1",
    );
    expect(document.querySelector<HTMLInputElement>("#runtime-model")!.value).toBe(
      "draft-model",
    );
    const password = document.querySelector<HTMLInputElement>("#runtime-api-key")!;
    expect(password.type).toBe("password");
    expect(password.value).toBe(RUNTIME_REPLACEMENT_KEY);
    expect(document.body.textContent).not.toContain(RUNTIME_REPLACEMENT_KEY);
    expect(document.body.textContent).toContain("已配置，末四位 1111");
  });

  it("closes on Escape, restores trigger focus, and never reuses a closed Key draft", async () => {
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies"
      ) {
        return [] as T;
      }
      if (command === "get_runtime_profile") {
        return runtimeProfileView() as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("运行时设置");
    await vi.waitFor(() => expect(document.querySelector("#runtime-api-key")).not.toBeNull());
    await setControlValue("#runtime-api-key", RUNTIME_REPLACEMENT_KEY);
    await act(async () => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });

    await vi.waitFor(() => {
      expect(document.querySelector('[aria-labelledby="runtime-settings-title"]')).toBeNull();
      expect(document.activeElement?.textContent).toBe("运行时设置");
    });
    await clickButton("运行时设置");
    await vi.waitFor(() => expect(document.querySelector("#runtime-api-key")).not.toBeNull());
    expect(document.querySelector<HTMLInputElement>("#runtime-api-key")!.value).toBe("");
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "get_runtime_profile"),
    ).toHaveLength(2);
  });
});

describe("S28 Windows activity timeline", () => {
  it("shows explicit gaps and pauses through whitelisted commands", async () => {
    const timeline: ActivityTimelineEntry[] = [
      {
        id: 1,
        kind: "activity",
        application: "code.exe",
        windowTitle: "S28",
        idle: false,
        gapReason: null,
        startedAtMillis: 1_785_000_000_000,
        observedUntilMillis: 1_785_000_001_000,
        endedAtMillis: 1_785_000_001_000,
      },
      {
        id: 2,
        kind: "gap",
        application: null,
        windowTitle: null,
        idle: null,
        gapReason: "crash",
        startedAtMillis: 1_785_000_001_000,
        observedUntilMillis: 1_785_000_002_000,
        endedAtMillis: null,
      },
    ];
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (
        command === "list_conversation" ||
        command === "list_shared_experience_ceremonies"
      ) {
        return [] as T;
      }
      if (command === "get_capture_status") {
        return { state: "collecting" } as T;
      }
      if (command === "list_activity_timeline") {
        return timeline as T;
      }
      if (command === "set_capture_paused") {
        return { state: "paused" } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("活动时间线");
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("code.exe · 活跃");
      expect(document.body.textContent).toContain("采集空缺 · 宿主崩溃");
      expect(document.body.textContent).toContain("不会推测或填补");
    });

    await clickButton("暂停记录");
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("set_capture_paused", {
        paused: true,
      }),
    );
    expect(document.body.textContent).toContain("当前状态：本人已暂停");
  });
});

describe("S20 typed shared-experience ceremony", () => {
  it("requires explicit confirmation before admitting a shared agreement", async () => {
    const ceremony = agreementCeremony();
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [ceremony] as T;
      }
      if (command === "resolve_shared_agreement") {
        return { candidateId: 7, status: "confirmed", claimId: 11 } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("共同约定待确认");
      expect(document.body.textContent).toContain("发现关键逃避时直接指出");
      expect(document.body.textContent).toContain("候选 v1");
      expect(document.body.textContent).toContain("双方的重要议题讨论");
      expect(document.body.textContent).toContain(
        "持续有效，直到任何一方退出或双方签署替代约定",
      );
      expect(document.body.textContent).toContain("我也同意");
    });

    await clickButton("确认入账");

    await vi.waitFor(() =>
      expect(document.body.textContent).not.toContain("共同约定待确认"),
    );
    expect(invokeMock).toHaveBeenCalledWith("resolve_shared_agreement", {
      candidateId: 7,
      confirm: true,
    });
  });

  it("creates a new immutable candidate version instead of editing the signable one", async () => {
    const ceremony = agreementCeremony();
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [ceremony] as T;
      }
      if (command === "revise_shared_agreement") {
        return { candidateId: 8, version: 2, status: "awaitingCounterpart" } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("共同约定待确认"));
    await clickButton("提出修改");
    await setControlValue("#agreement-statement", "只在正式复盘时直接指出关键逃避");
    await clickButton("提交新版本");

    await vi.waitFor(() => {
      expect(document.body.textContent).not.toContain("共同约定待确认");
      expect(document.body.textContent).toContain(
        "候选 v2 已生成，等待第二自我明确同意该精确版本",
      );
    });
    expect(invokeMock).toHaveBeenCalledWith("revise_shared_agreement", {
      candidateId: 7,
      statement: "只在正式复盘时直接指出关键逃避",
      scope: "双方的重要议题讨论",
      effectiveFromMillis: 1_785_000_000_000,
      effectiveUntilMillis: null,
      endCondition: null,
      supersedesAgreementIds: [],
    });
  });

  it("shows the complete whole-agreement supersession list before signing", async () => {
    const ceremony: SharedExperienceCeremony = {
      ...agreementCeremony(),
      statement: "复盘时不要直接指出关键逃避",
      supersededAgreements: [
        {
          claimId: 4,
          statement: "复盘时直接指出关键逃避",
          scope: "双方共同项目复盘",
          effectiveFromMillis: 1_785_000_000_000,
          effectiveUntilMillis: null,
        },
      ],
    };
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [ceremony] as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("本约定将整份取代以下共同约定");
      expect(document.body.textContent).toContain("Claim #4：复盘时直接指出关键逃避");
      expect(document.body.textContent).toContain("双方共同项目复盘");
      expect(document.body.textContent).toContain("不会推导任何残余义务");
    });
  });

  it("closes a non-veto disagreement notice without offering denial", async () => {
    const ceremony: SharedExperienceCeremony = {
      targetId: 13,
      targetKind: "sharedExperience",
      experienceKind: "substantiveDisagreement",
      admission: "nonVetoNotice",
      statement: "双方对这件事的重要性持不相容立场",
      candidateVersion: null,
      scope: null,
      effectiveFromMillis: null,
      effectiveUntilMillis: null,
      endCondition: null,
      agreementClaimId: null,
      departureReason: null,
      withdrawalActor: null,
      supersededAgreements: [],
      evidence: [
        { evidenceId: 1, speaker: "person", quote: "这件事无关紧要" },
        { evidenceId: 2, speaker: "counterpart", quote: "我不同意" },
      ],
    };
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [ceremony] as T;
      }
      if (command === "dismiss_shared_experience_ceremony") {
        return undefined as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("实质分歧已记录");
      expect(document.body.textContent).toContain("关闭通知不会撤销记录");
      expect(document.body.textContent).not.toContain("否认");
    });

    await clickButton("已知悉并关闭");

    await vi.waitFor(() =>
      expect(document.body.textContent).not.toContain("实质分歧已记录"),
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "dismiss_shared_experience_ceremony",
      { claimId: 13 },
    );
  });

  it("shows the departed agreement and exact reason as a non-veto shared event", async () => {
    const ceremony: SharedExperienceCeremony = {
      targetId: 14,
      targetKind: "sharedExperience",
      experienceKind: "agreementBreach",
      admission: "nonVetoNotice",
      statement:
        "偏离共同约定“直接指出关键逃避”：因为安全边界禁止现实行动授权",
      candidateVersion: null,
      scope: null,
      effectiveFromMillis: null,
      effectiveUntilMillis: null,
      endCondition: null,
      agreementClaimId: 7,
      departureReason: "因为安全边界禁止现实行动授权",
      withdrawalActor: null,
      supersededAgreements: [],
      evidence: [
        { evidenceId: 1, speaker: "person", quote: "我同意" },
        { evidenceId: 2, speaker: "counterpart", quote: "我也同意" },
        {
          evidenceId: 4,
          speaker: "counterpart",
          quote: "因为安全边界禁止现实行动授权",
        },
      ],
    };
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [ceremony] as T;
      }
      if (command === "dismiss_shared_experience_ceremony") {
        return undefined as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("共同约定偏离已记录");
      expect(document.body.textContent).toContain("Claim #7");
      expect(document.body.textContent).toContain("因为安全边界禁止现实行动授权");
      expect(document.body.textContent).toContain("关闭通知不会撤销记录");
    });

    await clickButton("已知悉并关闭");
    expect(invokeMock).toHaveBeenCalledWith(
      "dismiss_shared_experience_ceremony",
      { claimId: 14 },
    );
  });
});

describe("S24 asymmetric agreement withdrawal", () => {
  it("does not invoke withdrawal on cancel and confirms with an empty optional reason", async () => {
    const activeAgreement = {
      claimId: 7,
      statement: "复盘时直接指出关键逃避",
      scope: "双方共同项目复盘",
      effectiveFromMillis: 1_785_000_000_000,
      effectiveUntilMillis: null,
    };
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [] as T;
      }
      if (command === "list_active_shared_agreements") {
        return [activeAgreement] as T;
      }
      if (command === "withdraw_shared_agreement_as_person") {
        return 19 as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("从此刻继续认识彼此"));
    await clickButton("管理共同约定");
    await vi.waitFor(() => expect(document.body.textContent).toContain("复盘时直接指出关键逃避"));
    await clickButton("退出这项约定");
    await vi.waitFor(() => expect(document.body.textContent).toContain("确认退出共同约定"));
    await clickButton("取消");

    expect(invokeMock).not.toHaveBeenCalledWith(
      "withdraw_shared_agreement_as_person",
      expect.anything(),
    );

    await clickButton("退出这项约定");
    await clickButton("确认退出");
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("withdraw_shared_agreement_as_person", {
        agreementClaimId: 7,
        confirmed: true,
        reason: null,
      }),
    );
  });

  it("offers only acknowledgement or continued response for counterpart withdrawal", async () => {
    const ceremony: SharedExperienceCeremony = {
      targetId: 19,
      targetKind: "sharedExperience",
      experienceKind: "agreementWithdrawal",
      admission: "nonVetoNotice",
      statement:
        "第二自我退出共同约定“复盘时直接指出关键逃避”：它已妨碍我诚实表达独立判断",
      candidateVersion: null,
      scope: null,
      effectiveFromMillis: 1_785_000_100_000,
      effectiveUntilMillis: null,
      endCondition: null,
      agreementClaimId: 7,
      departureReason: "它已妨碍我诚实表达独立判断",
      withdrawalActor: "counterpart",
      supersededAgreements: [],
      evidence: [
        { evidenceId: 1, speaker: "person", quote: "我同意" },
        { evidenceId: 2, speaker: "counterpart", quote: "我也同意" },
        {
          evidenceId: 4,
          speaker: "counterpart",
          quote: "它已妨碍我诚实表达独立判断",
        },
      ],
    };
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [ceremony] as T;
      }
      if (command === "dismiss_shared_experience_ceremony") {
        return undefined as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("第二自我已退出共同约定");
      expect(document.body.textContent).toContain("它已妨碍我诚实表达独立判断");
      expect(document.body.textContent).toContain("另一方不能否决");
    });
    const actions = [...document.querySelectorAll(".ceremony-actions button")].map(
      (button) => button.textContent,
    );
    expect(actions).toEqual(["已知悉", "继续回应"]);
    expect(document.body.textContent).not.toMatch(/否决退出|阻止退出|撤销退出/);

    await clickButton("继续回应");
    await vi.waitFor(() =>
      expect(document.body.textContent).not.toContain("第二自我已退出共同约定"),
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "dismiss_shared_experience_ceremony",
      { claimId: 19 },
    );
    expect(document.activeElement).toBe(document.querySelector("#message"));
  });
});

describe("S25 immutable identity history", () => {
  it("shows trusted versions as read-only history without a person edit path", async () => {
    const identities: IdentityStateVersion[] = [
      identityVersion(1, null, "岚", "温和、克制", "基于初始自述形成"),
      identityVersion(
        2,
        1,
        "岚",
        "直白、审慎、不武断",
        "这更能保持独立判断，同时让提醒可被质疑。",
      ),
    ];
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [] as T;
      }
      if (command === "list_identity_history") {
        return identities as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith("list_identity_history"));
    await clickButton("身份版本");

    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("不可改写身份链");
      expect(document.body.textContent).toContain("v2 · 岚");
      expect(document.body.textContent).toContain("直白、审慎、不武断");
      expect(document.body.textContent).toContain(
        "这更能保持独立判断，同时让提醒可被质疑。",
      );
      expect(document.body.textContent).toContain("不能直接编辑这些版本");
    });
    const dialog = document.querySelector('[aria-labelledby="identity-history-title"]')!;
    expect(dialog.querySelector("input, textarea")).toBeNull();
    expect([...dialog.querySelectorAll("button")].map((button) => button.textContent)).toEqual([
      "关闭",
    ]);
  });
});

describe("S26 deferrable reflection invitation ceremony", () => {
  it("shows the one-time mute choice after a repeated deferral and preserves explicit semantics", async () => {
    const reflection = reflectionInvitation(true);
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [] as T;
      }
      if (command === "list_offered_reflection_invitations") {
        return [reflection] as T;
      }
      if (command === "decide_reflection_invitation") {
        return { invitationId: reflection.id, state: "mutedByPerson" } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("想和你一起看一件事");
      expect(document.body.textContent).toContain("这次工作节奏变化值得一起看");
      expect(document.body.textContent).toContain("刚发生且有直接依据");
      expect(document.body.textContent).toContain("第一次把工作节奏提快了");
      expect(document.body.textContent).toContain("观察与证据不会被删除");
    });
    expect([...document.querySelectorAll(".ceremony-actions button")].map(
      (button) => button.textContent,
    )).toEqual(["继续延后", "不再主动提起", "这次已谈完"]);

    await clickButton("不再主动提起");
    await vi.waitFor(() =>
      expect(document.body.textContent).not.toContain("想和你一起看一件事"),
    );
    expect(invokeMock).toHaveBeenCalledWith("decide_reflection_invitation", {
      invitationId: 26,
      decision: "mute",
    });
  });

  it("offers defer and resolve without prompting for mute on the first offer", async () => {
    const reflection = reflectionInvitation(false);
    mockReadyInvoke(async <T,>(command: string): Promise<T> => {
      if (command === "list_conversation") {
        return [] as T;
      }
      if (command === "list_shared_experience_ceremonies") {
        return [] as T;
      }
      if (command === "list_offered_reflection_invitations") {
        return [reflection] as T;
      }
      if (command === "decide_reflection_invitation") {
        return { invitationId: reflection.id, state: "deferred" } as T;
      }
      throw new Error(`unexpected command: ${command}`);
    });

    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(document.body.textContent).toContain("反思邀请"));
    expect([...document.querySelectorAll(".ceremony-actions button")].map(
      (button) => button.textContent,
    )).toEqual(["稍后再说", "这次已谈完"]);
    expect(document.body.textContent).not.toContain("不再主动提起");

    await clickButton("稍后再说");
    expect(invokeMock).toHaveBeenCalledWith("decide_reflection_invitation", {
      invitationId: 26,
      decision: "defer",
    });
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

async function clickButton(label: string) {
  const button = [...document.querySelectorAll("button")].find(
    (item) => item.textContent === label,
  );
  expect(button).toBeDefined();
  await act(async () => button!.click());
}

async function setControlValue(selector: string, value: string) {
  const control = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector)!;
  const prototype =
    control instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(prototype, "value")!.set!;
  await act(async () => {
    setter.call(control, value);
    control.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function runtimeProfileView(
  overrides: Partial<{
    baseUrl: string;
    model: string;
    apiKeyConfigured: boolean;
    apiKeyLastFour: string | null;
  }> = {},
) {
  return {
    baseUrl: "http://127.0.0.1:11434/v1",
    model: "gpt-oss-20b",
    apiKeyConfigured: true,
    apiKeyLastFour: "1111",
    ...overrides,
  };
}

function agreementCeremony(): SharedExperienceCeremony {
  return {
    targetId: 7,
    targetKind: "agreementCandidate",
    experienceKind: "agreement",
    admission: "confirmationRequired",
    statement: "发现关键逃避时直接指出",
    candidateVersion: 1,
    scope: "双方的重要议题讨论",
    effectiveFromMillis: 1_785_000_000_000,
    effectiveUntilMillis: null,
    endCondition: null,
    agreementClaimId: null,
    departureReason: null,
    withdrawalActor: null,
    supersededAgreements: [],
    evidence: [
      { evidenceId: 1, speaker: "person", quote: "我同意" },
      { evidenceId: 2, speaker: "counterpart", quote: "我也同意" },
    ],
  };
}

function reflectionInvitation(showMutePrompt: boolean): ReflectionInvitationCeremony {
  return {
    id: 26,
    topicKey: "work-rhythm",
    observation: "这次工作节奏变化值得一起看。",
    whyNow: "这是刚发生且有直接依据的重要变化。",
    importance: "important",
    basis: "importantSingleChange",
    deferCount: showMutePrompt ? 1 : 0,
    showMutePrompt,
    evidence: [
      {
        evidenceId: 8,
        speaker: "person",
        quote: "第一次把工作节奏提快了",
      },
    ],
  };
}

function turn(
  id: number,
  speaker: ConversationTurn["speaker"],
  verbatim: string,
): ConversationTurn {
  return { id, speaker, verbatim, recordedAtMillis: 1_785_000_000_000 + id };
}

function identityVersion(
  version: number,
  predecessorVersion: number | null,
  name: string,
  expressionTraits: string,
  changeReason: string,
): IdentityStateVersion {
  return {
    version,
    predecessorVersion,
    name,
    expressionTraits,
    viewpoints: "保留分歧",
    valuePriorities: "准确高于迎合",
    relationshipPosture: "同行者",
    ownGoals: "帮助本人看见长期变化",
    changeReason,
    evidenceIds: version === 1 ? [1, 2] : [7, 8],
    formedAtMillis: 1_785_000_000_000 + version,
  };
}
