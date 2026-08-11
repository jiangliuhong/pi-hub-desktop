# pi-hub-desktop

`pi-hub-desktop` 是 Pi Hub 的 macOS / iOS 客户端，产品显示名称为 **Pi Hub Client**。

它不重写 Pi Hub，而是负责：

- 管理多个 Pi Hub 服务；
- 直接访问可用的 HTTP/HTTPS Pi Hub；
- 自动建立 SSH Local Port Forward；
- 在 App 内受控 WebView 中使用现有 Pi Hub WebUI；
- 在 macOS 检测和管理本机 Pi Hub；
- 检查本机 Pi Hub 与 Pi 的基础运行环境。

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

## 文档

### 项目规则

- [AGENTS.md](./AGENTS.md)

### V1

- [V1 开发需求](./docs/requirements-v1.md)
- [V1 技术设计](./docs/design-v1.md)

### V2

- [V2 开发需求](./docs/requirements-v2.md)
- [V2 技术设计](./docs/design-v2.md)

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

| 工具 | 要求 |
|---|---|
| Node | 24，见 `.nvmrc` |
| Rust | stable，见 `rust-toolchain.toml` |
| macOS 构建 | macOS + Xcode |
| iOS 构建 | macOS + Xcode + Apple Rust targets |

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
