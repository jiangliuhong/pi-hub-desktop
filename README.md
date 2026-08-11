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

> 阶段：**V1 领域核心已实现**（Phase 1 / Phase 2 的连接、状态机、Profile、凭据、SSH Forward、Host Key、前端表单与连接流程）。SSH Forward 通过 Linux 上的真 SSH Server 集成测试验证；Apple Keychain 与 Service View 仍需真机验收。

### 已实现并验证（Linux 可运行部分）

- **错误模型**：`AppError` 类型枚举 + 稳定错误码 DTO，`details` 仅携带白名单非敏感字段，秘密永不出现在 DTO / 日志（`src-tauri/src/error.rs`，单测覆盖脱敏）。
- **服务配置**：`ServiceProfile` tagged enum（`direct_url` / `ssh_forward`）、原子版本化 Store（JSON 文件 + temp+rename）、向前迁移、服务 CRUD、凭据引用计数与孤儿清理（`profile/`，单测 + 临时文件持久化测试）。
- **凭据抽象**：`CredentialStore` trait + 纯内存实现（测试用）+ Apple Keychain 实现（`cfg(target_vendor = "apple")` 门控，结构完整）。Secret 在 Drop 时尽力清零。
- **连接状态机**：明确枚举与合法转换，非法转换在测试中报错（`connection/state.rs`）。
- **ConnectionManager**：每服务去重、RAII + `CancellationToken` 资源管理、Host Key 确认往返、显式断开释放（`connection/manager.rs`，单测）。
- **Provider 抽象**：`DirectUrlProvider` / `SshForwardProvider` 统一接口（UI 不接触 SSH，NFR-005）。
- **SSH**：`russh` 连接 + 严格 Host Key 校验（未知/变更一律阻断，绝不 `StrictHostKeyChecking=no`）、Password / Ed25519 / RSA + Passphrase、`127.0.0.1:0` 随机 loopback Listener、每连接独立 `direct-tcpip` + 双向 `copy_bidirectional`、keepalive、取消。
- **SSH 集成测试**（`src-tauri/tests/ssh_forward.rs`）：启动进程内真 `russh` server + mock Pi Hub，验证：未知 Host Key→确认→重连、错误密码拒绝、`direct-tcpip` 经隧道往返 HTTP、取消停走 accept loop。
- **Tauri 命令**：profile / credential / connection / viewer 薄适配层，状态图注入。
- **前端**：服务列表（FR-001）、条件字段表单 + 校验（FR-002/003）、Host Key 确认对话框（FR-007）、连接进度 + 诊断 + 重试（FR-009/016）、Viewer 受控工具栏。秘密仅在调用 Keychain 时短暂存在并立即清空。

### 检查状态（本 Linux 环境）

| 检查                                          | 状态                            |
| --------------------------------------------- | ------------------------------- |
| `npm run format:check` / `lint` / `typecheck` | ✅ 通过                         |
| `npm test`                                    | ✅ 35 个前端测试通过            |
| `npm run build`                               | ✅ Vite 构建成功                |
| `cargo fmt --check`                           | ✅ 干净                         |
| `cargo clippy --all-targets -- -D warnings`   | ✅ 干净                         |
| `cargo test`                                  | ✅ 53 单测 + 4 SSH 集成测试通过 |

### 仍需真机验收（AGENTS.md §12.4 / §15，如实记录）

- **Apple Keychain**：`apple_keychain.rs` 仅在 `target_vendor = "apple"` 编译，未在 macOS / iPhone 真机验证实际读写与 item 删除。
- **Service WebView**：独立零权限 WKWebView / WebviewWindow 的创建、Basic Auth challenge、Cookie 按 service 隔离、文件选择 / 下载、外部链接交给系统浏览器——命令已记录意图，原生窗口创建需在 macOS / iPhone 验证。
- **真机 SSH**：Password、Ed25519 Key、带 Passphrase Key 在 iPhone 真机的 `russh` 表现；iOS 前后台恢复。
- **平台构建**：`npm run tauri build` 与 `npm run tauri ios build` 需 macOS / Xcode，本环境无法执行。
- **HTTP Basic Auth**：Pi Hub 密码 Keychain 集成需真机验证（design §15 Spike D）。

以上未验证项不声称已完成。

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
