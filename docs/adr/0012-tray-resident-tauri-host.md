---
status: accepted
---

# Core 内嵌托盘常驻 Tauri 宿主

首版不部署独立 `core-host.exe`；当前用户下的 `evrything-about-me.exe` 是非提权单实例常驻宿主，Rust Core 内嵌其中。关闭窗口只隐藏到系统托盘，点击应用或托盘图标重新显示，只有显式“退出”才停止 Core。

## Considered Options

- 独立当前用户 Core：故障隔离更强，但在首版验证核心价值前引入桌面/Core IPC、双进程升级和恢复协调。
- Windows Service：Session 0 不适合当前用户桌面采集，否决。
- Core 内嵌托盘宿主：满足关窗后持续记录，并显著减少首版运行与部署边界，采纳。

## Consequences

宿主崩溃或升级会造成采集空缺，重启后必须恢复存储并标记空缺。WebView 只能使用白名单 Tauri command，Rust 领域能力继续保留在独立 crates 中，以便实际需要时再拆出后台进程。本决策取代 ADR-0010。
