import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { App } from "./App";

describe("S07 static host shell", () => {
  it("states that the host is ready without claiming conversation UI exists", () => {
    const markup = renderToStaticMarkup(<App />);

    expect(markup).toContain("桌面宿主已启动");
    expect(markup).toContain("持续对话界面将在下一切片接入");
  });
});
