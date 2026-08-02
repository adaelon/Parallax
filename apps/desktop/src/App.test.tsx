// @vitest-environment jsdom

import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  App,
  ConversationTurn,
  IdentityStateVersion,
  SharedExperienceCeremony,
} from "./App";

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
      ceremonies: [],
    };
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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

describe("S20 typed shared-experience ceremony", () => {
  it("requires explicit confirmation before admitting a shared agreement", async () => {
    const ceremony = agreementCeremony();
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
    invokeMock.mockImplementation(async <T,>(command: string): Promise<T> => {
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
