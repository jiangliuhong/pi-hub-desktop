# pi-hub-desktop

`pi-hub-desktop` 是 Pi Hub 的 macOS / iOS 客户端，产品显示名称为 **Pi Hub Client**。

V1 允许用户配置多个 Pi Hub 服务，并通过以下方式访问：

- Direct URL：直接打开可访问的 HTTP/HTTPS Pi Hub
- SSH Forward：自动建立 SSH Local Port Forward，再在 App 内打开 Pi Hub

用户不需要手动运行 SSH、不需要使用 Termius 建立映射，也不需要再切换到外部浏览器。

## V1 范围

- macOS 14+
- iOS 17+
- Direct URL
- SSH Password / OpenSSH Private Key
- SSH Host Key 校验
- 随机 loopback 本地端口
- App 内受控 WebView
- iOS 前后台恢复
- 对接已经存在的 SSH 中继端点

V1 不实现 Relay Server、家庭电脑常驻 Agent、Reverse Tunnel、Terminal、SFTP 或 VPN。

## 文档

- [开发规则](./AGENTS.md)
- [V1 开发需求](./docs/requirements-v1.md)
- [V1 技术设计](./docs/design-v1.md)

## 目标架构

```text
Pi Hub Client
├── DirectProvider ───────────────→ Pi Hub URL
└── SshForwardProvider
    └── 127.0.0.1:random ─ SSH ─→ target-host:target-port

Service WebView ────────────────→ Existing Pi Hub WebUI
```

远端 Pi Hub 页面与 Tauri 原生权限严格隔离，SSH Password、Private Key 和 Passphrase 只存入 Apple Keychain。

## 当前状态

项目处于 V1 设计与初始化阶段。正式开发前先完成以下技术验证：

1. Tauri 2 同仓构建 macOS 和 iOS。
2. iPhone 真机运行 `russh` 并完成 Local Port Forward。
3. 使用独立、零权限 Service WebView 加载 Pi Hub。
4. 在 iOS/macOS 上读写 Apple Keychain。
5. 验证 Pi Hub 流式输出、文件选择和前后台恢复。
