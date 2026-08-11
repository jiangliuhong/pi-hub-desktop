# AGENTS.md

本文件是 `pi-hub-desktop` 仓库内所有 AI 编码代理和开发者的最高优先级项目规则。开始修改代码前，必须先阅读本文件以及与任务对应的文档：

- `docs/requirements-v1.md`
- `docs/design-v1.md`
- `docs/requirements-v2.md`
- `docs/design-v2.md`

V1 文档定义远程 Pi Hub 连接能力；V2 文档定义 macOS 本机 Pi Hub 管理与 Pi 环境诊断。实现与文档冲突时，先修正文档或明确记录设计决策，不得静默扩大范围。

---

## 1. 项目定义

仓库名称为 `pi-hub-desktop`，产品显示名称为 **Pi Hub Client**。

本项目不是 Pi Hub 的重写，而是 Pi Hub 的 macOS / iOS 客户端：

### V1

1. 保存多个 Pi Hub 服务配置。
2. 直接打开可访问的 Pi Hub URL。
3. 自动建立 SSH Local Port Forward 后打开 Pi Hub。
4. 在 App 内使用现有 Pi Hub WebUI。

### V2

1. 在 macOS 检测本机 Node.js 与 Pi Hub 安装。
2. 检测本机 Pi Hub 的真实运行状态。
3. 启动、停止和重启由当前 Desktop 实例管理的 Pi Hub。
4. 检查 Pi Hub 内嵌 Pi Runtime、Agent 目录、Session、认证和模型基础状态。
5. 按用户设置在打开 Desktop 时自动启动本机 Pi Hub。

核心业务仍由 `pi-hub / pi-web` 提供。本仓库不得复制 Pi Hub 的会话、Agent、Scheduler、Workspace、模型或认证业务实现。

---

## 2. 固定平台与范围

### 2.1 平台

- macOS 14+
- iOS 17+

必须使用同一个 Tauri 2 工程和同一套 React / TypeScript 前端。共享逻辑优先放在 Rust 中，平台差异通过明确适配层处理。

V2 本机 Runtime 管理只在 macOS 生效；iOS 必须继续编译并保持 V1 功能，不得启动本地进程或显示本机 Runtime 管理入口。

### 2.2 V1 服务类型

只允许：

- `direct_url`
- `ssh_forward`

`ssh_forward` 可以连接已经存在的 SSH 中继端点，但本仓库当前不负责创建中继端点。

### 2.3 当前版本不做

禁止未经需求变更自行加入：

- Relay Server
- 家庭电脑常驻 Relay Agent
- SSH Reverse Forward
- ProxyJump / 多级跳板
- SSH Terminal、PTY、SFTP
- VPN、系统代理或通用端口转发器
- Android、Windows、Linux
- 用户账号、云同步、团队协作
- Pi Hub WebUI 重写
- 后台永久维持 iOS Tunnel
- macOS 登录项、LaunchAgent 或 LaunchDaemon
- Desktop 退出后长期常驻 Pi Hub
- 自动安装或升级 Node.js、Pi、Pi Hub
- 任意 Shell 启动命令或环境变量编辑器
- Mac App Store 沙盒适配

发现这些需求时，应记录到后续版本，不得顺手实现。

---

## 3. 技术栈

除非经过明确设计变更，否则使用：

- Tauri 2
- React
- TypeScript strict
- Vite
- Rust stable
- Tokio
- `russh`：SSH 客户端和 `direct-tcpip`
- Apple Security Framework / Keychain：敏感凭据
- Rust 侧版本化 Store：非敏感配置

不要仅为方便增加 Electron、Flutter、第二套 UI 工程或通用 Node sidecar。

依赖必须固定兼容版本并提交 lockfile。新增依赖前必须说明：

1. 解决的问题。
2. 为什么标准库或现有依赖不能解决。
3. 是否支持 macOS、`aarch64-apple-ios` 和 iOS Simulator。
4. 安全与维护风险。

---

## 4. 目录边界

优先保持：

```text
pi-hub-desktop/
├── src/
│   ├── app/
│   ├── features/
│   │   ├── services/
│   │   ├── connection/
│   │   ├── viewer/
│   │   └── local-runtime/       # V2 macOS UI
│   ├── components/
│   ├── lib/
│   └── types/
├── src-tauri/
│   ├── capabilities/
│   ├── src/
│   │   ├── commands/
│   │   ├── connection/
│   │   ├── credential/
│   │   ├── profile/
│   │   ├── ssh/
│   │   ├── viewer/
│   │   ├── local_runtime/       # V2 本机 Runtime 领域
│   │   ├── platform/
│   │   ├── error.rs
│   │   └── lib.rs
│   └── tauri.conf.json
├── docs/
├── AGENTS.md
└── README.md
```

不提前拆复杂 monorepo 或大量 crates。只有模块边界稳定且存在真实复用需求时才能拆 crate。

---

## 5. 架构规则

### 5.1 远程连接 Provider

UI 不得直接操作 SSH：

```text
ConnectionProvider
├── DirectUrlProvider
└── SshForwardProvider
```

未来新增 Relay 时应增加 Provider，而不是修改 UI 核心流程。

### 5.2 本机 Runtime 独立领域

本机 Pi Hub 不是普通 `DirectUrlProfile`。

```text
LocalRuntimeManager
├── InstallationDetector
├── PiEnvironmentDoctor
├── LocalServiceProbe
├── ProcessSupervisor
├── LocalRuntimeSettingsStore
└── RuntimeLogBuffer
```

不得把安装扫描、进程管理或 Doctor 逻辑塞进 V1 `ConnectionManager`。

### 5.3 Command 必须薄

Tauri command 只负责：

- 输入校验
- 调用领域服务
- 转换错误
- 返回可序列化 DTO

禁止在 command 中堆积 SSH、进程、存储、Doctor 或生命周期逻辑。

### 5.4 Rust 保存真实状态

SSH Session、本地 Listener、Managed Child、运行状态和操作 generation 的唯一真实状态必须在 Rust 侧。

前端只展示 Rust 返回或事件推送的状态，不得仅靠 React state 判断 Tunnel 或进程是否存在。

### 5.5 跨仓库契约

V2 允许因明确需求修改 `jiangliuhong/pi-hub`，但必须先实现并测试稳定协议：

```text
pi-hub --version --json
pi-hub doctor --json --offline
GET /api/client-info
SIGTERM / SIGINT 子进程转发
```

Desktop 不得以解析 `Ready` 文本、扫描 Pi Hub `node_modules` 内部结构或猜测端口内容作为长期方案。

---

## 6. 安全硬规则

以下规则没有“先做出来再说”的例外。

### 6.1 凭据

以下数据只能存入 Apple Keychain：

- SSH Password
- SSH Private Key
- Private Key Passphrase
- Pi Hub HTTP 凭据

服务和 Runtime 设置只能保存 `credential_id`。禁止秘密出现在：

- JSON / Store / SQLite 明文字段
- Git 和测试快照
- panic 和日志
- URL query / user-info
- Tauri event payload
- Doctor JSON
- 完整环境变量转储

### 6.2 SSH Host Key

- 首次连接显示算法和 SHA-256 Fingerprint。
- Host Key 按 `host + port` 绑定。
- Host Key 变化时阻断连接。
- 禁止自动接受变化后的 Host Key。
- 禁止任何等价于 `StrictHostKeyChecking=no` 的逻辑。

### 6.3 SSH 本地映射

- 只绑定 `127.0.0.1`。
- 使用系统临时端口。
- 失败、断开、页面关闭和 App 退出时释放 Listener。
- 每个连接有独立生命周期。

### 6.4 Service WebView

Pi Hub WebView 按不可信内容处理：

- 不配置任何 remote capability。
- 不启用 `dangerousRemoteDomainIpcAccess`。
- 不得读取 Keychain、Store、文件或 Runtime 状态。
- 不得调用 Shell、Process、Dialog 或任何 Tauri Command。
- 外部链接默认交给系统浏览器。
- 只允许当前服务 origin 和经过验证的内部导航。

新增 V2 Runtime Commands 后，也不得授予 Service WebView 调用权限。

### 6.5 HTTP / TLS

- Direct URL 的 HTTP 必须显示明文警告。
- HTTPS 使用系统证书校验。
- 不提供忽略证书错误。
- 不全局关闭 ATS，只配置必要的最小 loopback 例外。
- Desktop 管理的本机 Pi Hub 强制绑定 `127.0.0.1`，不提供 `0.0.0.0` 设置。

### 6.6 本机进程

- 禁止 `sh -c`、`zsh -c` 和字符串拼接命令。
- 禁止向前端暴露通用 `execute`、`spawn`、`kill`。
- 使用经过验证的绝对 Node.js 与 Pi Hub 入口路径。
- 启动参数由 Rust 固定构造。
- 只终止当前 App 内存中持有 Child Handle 的进程组。
- 不根据端口、进程名或 PID 猜测进程所有权。

---

## 7. SSH 实现规则

V1 支持：

- Password authentication
- OpenSSH Ed25519 / RSA private key
- 加密私钥和 Passphrase
- SSH keepalive
- `direct-tcpip` Local Port Forward

不支持：

- SSH Agent
- FIDO2 / YubiKey
- PKCS#11
- Remote Forward
- Dynamic Forward / SOCKS
- Shell / PTY

每个本地 TCP 连接对应一个 `direct-tcpip` channel。转发必须双向异步、支持 EOF、取消和资源释放；不得记录业务数据。禁止固定 sleep 判断 ready。

---

## 8. V2 本机 Runtime 规则

### 8.1 Pi 依赖定义

Pi Hub 内嵌 `@earendil-works/pi-coding-agent` 等 Pi Runtime。独立的全局 `pi` CLI 不是 Pi Hub 启动硬依赖。

- 内嵌 Pi Runtime 加载失败：required / blocked。
- 外部 `pi` CLI 未安装：informational，不阻止启动。
- 禁止仅凭 `command -v pi` 失败判定 Pi Hub 不可用。

### 8.2 安装发现

至少支持：

- 已保存绝对路径
- App PATH
- Homebrew
- `/usr/local`
- NVM
- Volta
- FNM
- ASDF / Mise shim

候选必须 canonicalize，并验证 package name、bin entry、version、Node engine 和生产构建。Finder 启动时不能依赖 `.zshrc`。

### 8.3 Doctor

`pi-hub doctor --json --offline` 必须：

- 不启动 HTTP Server。
- 不进入交互模式。
- 不调用模型 API。
- 不刷新 OAuth。
- 不输出 Credential。
- 不调用 `pi auth ... --credentials`。
- 返回带 schema version 的稳定 JSON 和退出码。

Desktop 只解析白名单字段，不把 stdout 原文直接暴露给 UI。

### 8.4 服务身份

判断本机端口时必须调用 `/api/client-info` 并校验 `service` 与 `protocolVersion`。

- TCP 不通：stopped。
- 返回有效 Pi Hub：running managed/external。
- 端口可用但不是 Pi Hub：port conflict。

不得仅以端口可连接或 stdout 包含 `Ready` 作为成功依据。

### 8.5 启动

实际等价于：

```text
<absolute-node>
<absolute-pi-hub-entrypoint>
--hostname 127.0.0.1
--port <configured-port>
--no-open
```

- 设置 package root 为 cwd。
- Node 目录置于 PATH 首位。
- 不执行用户 shell 配置。
- 可选 `PI_CODING_AGENT_DIR` 来自受控设置。
- 可选 `PI_HUB_PASSWORD` 从 Keychain 注入。
- 不记录完整环境。

### 8.6 进程所有权

- `running_managed`：当前 App 持有 Child Handle，可停止和重启。
- `running_external`：服务存在但无 Handle，只能打开。
- App 崩溃或重启后无法证明所有权的进程按 external 处理。
- 禁止端口反查 PID 后直接 Kill。

### 8.7 自动启动

“自动启动”只表示打开 Desktop 时拉起本机 Pi Hub，不等于开机启动。

- 设置默认关闭。
- 不阻塞主窗口显示。
- 已运行时不重复启动。
- blocked / port conflict 时不启动。
- 连续失败必须有崩溃循环保护。
- 自动启动失败不得影响远程服务使用。

### 8.8 退出

设置允许在真正退出 App 时停止本次受管服务，默认开启。

- 关闭窗口但 App 仍运行时不停止。
- 外部服务不处理。
- 优雅停止超时后才强制终止受管进程组。

### 8.9 日志

- 有固定行数和文件大小上限。
- 默认只保留最近启动日志。
- 不记录对话、HTTP 业务数据或完整环境。
- 必须对 Authorization、API Key、Cookie、Token、Private Key 等脱敏。
- 不允许主动打印 Secret 后只依赖正则擦除。

---

## 9. 生命周期规则

### macOS

- V1 SSH 连接可持续到用户断开或 App 退出。
- V2 本机 Runtime 按设置在 App 打开时检查/启动。
- 退出时释放 SSH、Listener 和受管进程资源。
- 当前版本不注册 LaunchAgent。

### iOS

- 前台维持连接。
- 不承诺后台持续 SSH。
- 回前台检测并重连。
- 不使用伪造后台模式。
- V2 本机 Runtime 命令返回 `unsupported_platform`，UI 隐藏入口。

---

## 10. 错误与状态

所有状态必须使用明确枚举，不得散落字符串。

V1 连接状态至少包括：

```text
idle
validating
connecting_ssh
verifying_host_key
authenticating
opening_forward
checking_service
connected
reconnecting
disconnecting
disconnected
error
```

V2 本机状态至少包括：

```text
unknown
checking
stopped
starting
running_managed
running_external
stopping
port_conflict
failed
```

错误必须提供稳定错误码和下一步建议。Rust 业务路径禁止 `unwrap()`、`expect()` 和无说明 panic；可恢复错误统一使用 typed error。

---

## 11. 前端规则

- TypeScript strict，禁止无说明 `any`。
- iOS 和 macOS 使用响应式共享 UI。
- 所有异步按钮有 loading、取消或防重复提交。
- 高风险操作必须二次确认。
- UI 不保存进程或 SSH 真相。
- 普通用户界面不展示 SSH 命令或 Shell 命令。
- macOS 服务列表顶部使用固定 `This Mac` 卡片，不把它保存为远程 Profile。
- blocked 时禁止启动；degraded 时允许启动但明确提示。
- external 状态默认不显示停止和重启操作。

---

## 12. 数据与迁移

- ID 使用稳定 UUID。
- 所有配置包含 `schema_version`。
- 结构变化提供向前迁移，不清空用户配置。
- 敏感字段不进入序列化模型。
- V1 Profiles 与 V2 Local Runtime Settings 使用独立存储和迁移。
- 安装路径保存前 canonicalize，使用前重新验证。

---

## 13. 测试规则

### V1

继续维护：

- Profile、URL、状态机、Host Key、凭据引用测试。
- 真 SSH Server 的认证与 `direct-tcpip` 集成测试。
- 前端服务 CRUD、状态、错误和 Viewer 测试。
- macOS / iPhone 真机 SSH、Keychain、WebView 验收。

### V2 单元与集成

至少覆盖：

- Homebrew / NVM / Volta / FNM 路径发现。
- symlink 与 package identity 验证。
- Node version requirement。
- Doctor schema、聚合和 Secret 脱敏。
- Agent / Session 目录权限。
- 本机状态机和 operation generation。
- mock client-info server。
- child early exit、启动超时、优雅停止、强制停止。
- duplicate start、port conflict、external refusal。
- crash-loop protection。
- 日志限制和 redaction。

### macOS 真机

至少验证：

- Homebrew Node + npm Pi Hub。
- NVM。
- Volta 或 FNM 至少一项。
- Finder 启动签名 App。
- 外部终端 Pi Hub。
- 非 Pi Hub 端口冲突。
- Node 版本过低。
- Agent 目录不可写。
- 无认证 / 无模型。
- `Cmd + Q`。
- Developer ID 签名、公证和 DMG。

### iOS 回归

V2 合并后仍需验证 iOS build、Direct URL、SSH Forward、Host Key、Viewer 和 Keychain。

涉及 SSH、Keychain、WebView、进程或生命周期的修改，必须说明实际验证平台。

---

## 14. 开发流程

1. 确认任务属于 V1 或 V2。
2. 阅读对应需求与设计章节。
3. 检查现有实现和测试。
4. 跨仓库契约先在 Pi Hub 实现，不写临时猜测逻辑。
5. 先完成最小闭环，不做无需求重构。
6. 增加或更新测试。
7. 执行格式化、类型检查、Lint 和测试。
8. 更新行为、限制和设计决策文档。
9. 提交说明写清已验证平台和未验证项。

不得为了让检查通过而：

- 删除测试
- 降低 TypeScript 严格度
- 大范围 lint ignore
- 跳过 Host Key
- 关闭 TLS 校验
- 给 Service WebView 增加权限
- 把 Secret 写入测试或日志
- 使用固定 sleep 伪造 ready
- Kill 不属于当前 App 的进程
- 把外部 `pi` CLI 变成错误的硬依赖

---

## 15. 预期检查命令

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
npm run tauri ios build
```

证书或真机环境不足时可以无法执行最后两项，但必须明确说明，不能声称完成验证。

涉及 `jiangliuhong/pi-hub` 的 V2 契约时，还必须运行该仓库的 typecheck、lint、test 和发布前构建检查。

---

## 16. Definition of Done

任务只有同时满足以下条件才算完成：

- 行为符合当前版本需求和技术设计。
- V1 与 V2 模块边界没有被破坏。
- macOS 和 iOS 共享逻辑未被无理由分叉。
- 安全硬规则未被破坏。
- 错误、取消、超时和资源释放得到处理。
- 只有明确受管的进程可被停止。
- Doctor 无网络副作用且不泄露 Secret。
- 新行为有对应测试。
- 相关文档已更新。
- 所执行和未执行的检查均被如实记录。

若 Tauri、`russh`、WKWebView、Keychain、进程组或 Pi Hub Doctor 契约存在技术阻塞，先提交可复现结论和替代方案，不得伪造完成状态。
