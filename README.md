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

## 技术栈

- Tauri 2（同一工程支持 macOS / iOS，使用系统 WKWebView）
- React + TypeScript（strict）+ Vite
- Rust（stable）+ Tokio
- `russh`（SSH 客户端与 `direct-tcpip`）
- Apple Keychain（凭据）+ `tauri-plugin-store`（非敏感配置）

依赖版本在初始化时固定并提交 `package-lock.json` 与 `src-tauri/Cargo.lock`。

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

> 阶段：**项目初始化完成**（V1 Phase 1 的第一步）。尚未进入需求开发。

已完成：

- Tauri 2 工程骨架（React + TypeScript strict + Vite），同一仓支持 macOS / iOS。
- 目录结构对齐 `docs/design-v1.md §5`（前端 `app/features/components/lib/types`，Rust `commands/connection/credential/profile/ssh/viewer/platform/error/event`）。
- Trusted App Shell 路由与占位页（服务列表、表单、连接、Viewer）。
- 前端工具链：Prettier、ESLint（flat config，type-checked）、Vitest + Testing Library。
- Rust 骨架：Tauri builder、模块边界、最小单测；`tauri.conf.json` 设置最低系统版本（macOS 14 / iOS 17）。
- Capability 仅绑定可信 `main` 窗口，远端 Service WebView 不匹配任何 capability（`AGENTS.md §6.4`）。
- 应用图标占位、`package-lock.json`、`src-tauri/Cargo.lock`、`rust-toolchain.toml`、`.nvmrc`。

尚未实现（按 V1 Phase 1/2 落地）：服务数据模型与 Store、Keychain 抽象、SSH、连接状态机、Service View 集成，以及 Phase 0 的真机技术验证。

正式开发前仍需完成的 Phase 0 技术验证：

1. Tauri 2 同仓构建 macOS 和 iOS。
2. iPhone 真机运行 `russh` 并完成 Local Port Forward。
3. 使用独立、零权限 Service WebView 加载 Pi Hub。
4. 在 iOS/macOS 上读写 Apple Keychain。
5. 验证 Pi Hub 流式输出、文件选择和前后台恢复。

## 开发环境

| 工具     | 要求                                                    |
| -------- | ------------------------------------------------------- |
| Node     | 24（见 `.nvmrc`）                                       |
| Rust     | stable（见 `rust-toolchain.toml`）                      |
| iOS 构建 | macOS + Xcode + Apple 目标三联 (`aarch64-apple-ios` 等) |

安装依赖：

```bash
npm install
```

> 注意：若当前 shell 的 `NODE_ENV=production`，需以 `NODE_ENV=development npm install` 安装 devDependencies。
>
> 该环境会对包脚本做白名单校验，首次安装后需批准 esbuild 的 postinstall：
> `npm approve-scripts esbuild`（Vite/Vitest 依赖其原生二进制）。

## 常用检查命令

对应 `AGENTS.md §14`。前端：

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
npm run build
```

Rust（在仓库根执行）：

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

平台构建（需要对应平台工具链，本仓库 Linux 环境无法执行）：

```bash
npm run tauri build        # macOS / 当前桌面平台
npm run tauri ios build    # iOS（需 macOS + Xcode）
```

## iOS 初始化

iOS 工程在首次构建前需在 macOS 上生成（产物在 `src-tauri/gen/apple`，已被 git 忽略）：

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
npm run tauri ios init
npm run tauri ios dev      # 模拟器
npm run tauri ios build    # 真机 / TestFlight
```

`tauri.conf.json` 已声明最低系统版本（macOS 14.0 / iOS 17.0）。
