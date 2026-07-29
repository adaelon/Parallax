---
status: accepted
---

# 第一版采用 Tauri、React 和 Rust

Windows 桌面壳采用 Tauri 2，界面使用 React 与 TypeScript，本地可信核心使用 Rust。该组合让保险库、Windows API、采集和策略停留在 Rust 核心，WebView 仅通过受限类型化命令交互。

## Considered Options

- .NET 与 WinUI 3：Windows 原生整合强，但当前环境无 SDK，且浏览器扩展仍需另一套技术。
- Electron 与 TypeScript：开发快，但完整人生保险库需要承担更大的 Node、IPC 与 Chromium 安全面。
- Tauri 2、React/TypeScript 与 Rust：开发成本较高，但最贴合既定可信边界，采纳。

## Consequences

Rust Core 是唯一特权组件；UI 不持有密钥或直接打开保险库，浏览器扩展独立构建，具体库版本在项目骨架切片锁定。
