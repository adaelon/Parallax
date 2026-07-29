---
status: superseded by ADR-0012
---

# Core 采用当前用户后台进程

`core-host.exe` 作为当前登录用户下的非提权单实例后台进程，在 Windows 登录后启动，并在 Tauri 窗口关闭后继续负责采集、保险库和记忆任务。Tauri 桌面程序是无密钥客户端；只有本人显式暂停采集或退出 Core 才改变后台状态。

## Considered Options

- Core 内嵌 Tauri 托盘进程：实现较少，但采集、密钥和数据库生命期与界面崩溃及升级耦合。
- Windows Service：可无人值守，但 Session 0 不适合观察当前用户桌面，也会复杂化当前用户 DPAPI 边界。
- 当前用户后台 Core：保持用户会话能力并使 UI 与记录生命期解耦，采纳。

## Consequences

Core 是保险库唯一写者和解锁后 Vault Key 的唯一持有者；UI 开关不影响采集。系统需要版本化本地 IPC、单实例启动和升级协调；具体传输协议另行决策。
