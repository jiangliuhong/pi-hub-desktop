# pi-hub-desktop

`pi-hub-desktop` 是 Pi Hub 的 macOS / iOS 客户端，产品显示名称为 **Pi Hub Client**。

它不重写 Pi Hub，而是负责：

- 管理多个 Pi Hub 服务；
- 直接访问可用的 HTTP/HTTPS Pi Hub；
- 自动建立 SSH Local Port Forward；
- 在 App 内受控 WebView 中使用现有 Pi Hub WebUI；
- 在 macOS 检测和管理本机 Pi Hub；
- 检查本机 Pi Hub 与 Pi 的基础运行环境；
- 在 macOS 受管安装/更新本机 Pi 与 Pi Hub（V3）。

## 版本范围

### V1：远程服务连接

支持 macOS 14+ 和 iOS 17+：

- Direct URL；
- SSH Password / OpenSSH Private Key；
- SSH Host Key 校验；
- 随机 loopback 本地端口；
- App 内受控 Service WebView；
- iOS 前后台恢复；
- 访问已经存在的 SSH 中继端点。

用户不需要手动运行 SSH、不需要使用 Termius 建立映射，也不需要再切换到外部浏览器。

### V2：本机 Pi Hub 管理与 Pi 环境检查

V2 主要面向 macOS：

- 自动发现 Node.js 和 `@jarome/pi-hub`；
- 检测本机 Pi Hub 是否运行；
- 区分 Desktop 管理的进程、外部进程和端口冲突；
- 在 Desktop 中启动、停止和重启受管 Pi Hub；
- 设置打开 Desktop 时自动启动；
- 检查 Pi Hub 内嵌 Pi Runtime、Agent 目录、Session、认证和模型基础状态；
- 显示有限、脱敏的启动日志。

V2 中独立的全局 `pi` CLI 是可选检查项，不是 Pi Hub 启动的硬依赖。Pi Hub 本身使用其安装包内的 Pi Runtime。

iOS 继续保留 V1 客户端能力，不实现本机进程管理。

### V3：本机 Pi / Pi Hub 组件管理

V3 在 V2 之上增量，只面向 macOS：

- 分别检测 Pi 与 Pi Hub 的安装、来源、版本与更新状态；
- 联网检查 stable 最新版本（缓存、离线降级、不降级）；
- 在 Desktop 受管目录内安装或更新 Pi / Pi Hub，不修改用户外部环境，不请求 `sudo`；
- 安装/更新使用固定包名、固定参数、原子激活、失败回滚与有限脱敏日志；
- 与 V2 Local Runtime 启停、Doctor、外部进程识别集成。

Node.js 自动安装/升级、beta 通道、后台静默更新、覆盖外部全局安装、写入系统目录均为 V3 非目标。iOS 不提供本机组件管理。

iOS 继续保留 V1 / V2 客户端能力，不实现本机组件包管理。

## 文档

### 项目规则

- [AGENTS.md](./AGENTS.md)

### V1

- [V1 开发需求](./docs/requirements-v1.md)
- [V1 技术设计](./docs/design-v1.md)

### V2

- [V2 开发需求](./docs/requirements-v2.md)
- [V2 技术设计](./docs/design-v2.md)

### V3

- [V3 开发需求](./docs/requirements-v3.md)
- [V3 技术设计：Pi 与 Pi Hub 包管理](./docs/pi-and-pi-hub-package-management-design.md)

## 架构

```text
Pi Hub Client
├── Remote Service Domain
│   ├── DirectUrlProvider ─────────────→ Pi Hub URL
│   └── SshForwardProvider
│       └── 127.0.0.1:random ─ SSH ───→ target-host:target-port
│
└── Local Runtime Domain (macOS V2)
    ├── InstallationDetector
    ├── PiEnvironmentDoctor
    ├── LocalServiceProbe
    ├── ProcessSupervisor
    └── LocalRuntimeSettingsStore

└── Package Management Domain (macOS V3)
    ├── InstallationDetector (复用/扩展 V2)
    ├── ReleaseClient
    ├── NpmToolchainDetector
    ├── PackageInstaller
    ├── PostInstallVerifier
    └── ManagedPackageStore ──activate──→ Local Runtime Domain

Service WebView ───────────────────────→ Existing Pi Hub WebUI
```

Pi Hub Service WebView 与 Tauri 原生权限严格隔离。SSH Password、Private Key、Passphrase 和 Pi Hub HTTP Password 只存入 Apple Keychain。

## V2 跨仓库契约

实现 V2 前，`jiangliuhong/pi-hub` 需要提供稳定的机器可读能力：

```text
pi-hub --version --json
pi-hub doctor --json --offline
GET /api/client-info
SIGTERM / SIGINT 子进程转发
```

Desktop 不通过解析 `Ready` 文本、猜测端口内容或扫描 Pi Hub 内部 `node_modules` 结构实现长期集成。

## 技术栈

- Tauri 2
- React + TypeScript strict + Vite
- Rust stable + Tokio
- `russh`
- Apple Keychain
- 版本化本地 Store

依赖版本固定并提交：

- `package-lock.json`
- `src-tauri/Cargo.lock`

## 当前仓库结构

```text
src/
├── app/
├── components/
├── features/
├── lib/
└── types/

src-tauri/src/
├── commands/
├── connection/
├── credential/
├── platform/
├── profile/
├── ssh/
├── viewer/
├── error.rs
└── lib.rs
```

V2 将在现有边界上新增：

```text
src/features/local-runtime/
src-tauri/src/local_runtime/
src-tauri/src/commands/local_runtime.rs
```

## 安全原则

- Service WebView 没有任何 Tauri remote capability。
- SSH Host Key 首次连接必须确认，发生变化必须阻断。
- SSH 映射只监听 `127.0.0.1`。
- V2 本机 Pi Hub 只绑定 `127.0.0.1`。
- 不向前端开放通用 Shell、Spawn 或 Kill。
- 只停止当前 Desktop 实例明确持有 Child Handle 的进程。
- 外部终端启动的 Pi Hub 默认只能打开，不能由 Desktop 强制停止。
- Pi Doctor 默认离线，不调用模型 API、不刷新 OAuth、不输出凭据。
- 日志不得出现 API Key、Token、Cookie、Private Key、Passphrase 或完整环境变量。

## 开发环境

| 工具       | 要求                               |
| ---------- | ---------------------------------- |
| Node       | 24，见 `.nvmrc`                    |
| Rust       | stable，见 `rust-toolchain.toml`   |
| macOS 构建 | macOS + Xcode                      |
| iOS 构建   | macOS + Xcode + Apple Rust targets |

安装依赖：

```bash
npm install
```

若当前 shell 设置了 `NODE_ENV=production`：

```bash
NODE_ENV=development npm install
```

## 常用检查

前端：

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
npm run build
```

Rust：

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

平台构建：

```bash
npm run tauri build
npm run tauri ios build
```

## macOS Release

向 `main` 推送版本标签后，GitHub Actions 会自动构建并发布两个 macOS DMG：

```bash
git tag v0.1.0
git push origin v0.1.0
```

- `aarch64`：Apple Silicon（M1/M2/M3/M4）
- `x86_64`：Intel Mac

workflow 位于 `.github/workflows/release-macos.yml`。当前默认构建未签名包；如需让 Finder 直接信任应用，还需要在 GitHub Secrets 配置 Apple Developer 签名和公证凭据。

平台构建和真机验收必须在具备 macOS、Xcode、签名与设备环境的机器上执行。缺少环境时必须如实记录，不能声称已验证。

## iOS 初始化

首次构建前在 macOS 上执行：

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
npm run tauri ios init
npm run tauri ios dev
npm run tauri ios build
```

`tauri.conf.json` 已声明最低系统版本：

- macOS 14.0
- iOS 17.0

## V2 实现进度与验证状态

### 已实现（Desktop 侧）

- 本机 Runtime 领域层（`src-tauri/src/local_runtime/`）：`model`、`settings`、`detector`、`doctor`、`health`、`logs`、`redaction`、`process`、`manager`。
- Tauri 命令（`commands/local_runtime.rs`）：状态、扫描、验证、Doctor、启动、停止、重启、设置、日志、平台支持判定。
- 生命周期：App 打开时异步初始化 + 可选自动启动 + 崩溃循环保护；`ExitRequested` 时按设置停止受管进程。
- 前端（`src/features/local-runtime/`）：This Mac 卡片、设置页、Environment Doctor 页、日志页、路由、标签与类型。
- iOS 门控：本机 Runtime 命令返回 `unsupported_platform`，UI 隐藏入口（`get_local_runtime_platform_support`）。

### 已执行的检查

| 检查                                        | 结果                          |
| ------------------------------------------- | ----------------------------- |
| `npm run format:check`                      | 通过                          |
| `npm run lint`                              | 通过                          |
| `npm run typecheck`                         | 通过                          |
| `npm test`（前端）                          | 45 通过                       |
| `npm run build`（Vite 产物）                | 通过                          |
| `cargo fmt --check`                         | 通过                          |
| `cargo clippy --all-targets -- -D warnings` | 通过                          |
| `cargo test`（Rust）                        | 119 通过（115 单元 + 4 集成） |

Rust 单元测试覆盖：脱敏、日志环形缓冲、设置迁移与原子写入、安装发现与包身份验证、Node 版本基线、Doctor JSON 解析与聚合、HTTP 探测（含 chunked 与 client-info 协议）、进程组启停、状态机、崩溃循环保护。

### 未验证项（需真机 / 跨仓库环境）

以下在当前环境（Linux 开发机）无法执行，必须在 macOS / iOS 真机环境验证后才能声称完成（AGENTS.md §13、§15、§16）：

- `npm run tauri build`（需 macOS + 代码签名）。
- `npm run tauri ios build`（需 macOS + Xcode + Apple Rust target）。
- macOS 真机矩阵：Homebrew / NVM / Volta / FNM、Finder 启动、外部进程、端口冲突、Node 过低、Agent 目录不可写、无认证 / 无模型、`Cmd + Q`、签名公证后的 DMG。
- iOS 回归：Direct URL、SSH Forward、Host Key、Viewer、Keychain、本机 Runtime 入口不可见。
- Pi Hub 跨仓库契约（`--version --json`、`doctor --json --offline`、`/api/client-info`、SIGTERM/SIGINT 转发）须在 `jiangliuhong/pi-hub` 实现并测试稳定后才能进入端到端真机验收。

Desktop 侧 Doctor 解析器已按 design-v2 §8.3 的 schema 实现，并包含 schemaVersion / 退出码一致性校验；在真实 Doctor 输出可用前，无法通过实际调用验证聚合结果。
