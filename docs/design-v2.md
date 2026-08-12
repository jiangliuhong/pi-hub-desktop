# Pi Hub Client V2 技术设计

- 对应需求：[`requirements-v2.md`](./requirements-v2.md)
- 文档版本：2.0
- 目标版本：V2 / `0.2.x`
- 主要平台：macOS 14+
- 兼容平台：iOS 17+ 保持 V1 能力
- 状态：设计基线

---

## 1. 设计目标

V2 在现有 Tauri 2 + React + Rust 架构中增加一个 macOS 专属的本机 Runtime 管理域：

```text
Pi Hub Client
├── Remote Service Domain               # V1
│   ├── DirectUrlProvider
│   └── SshForwardProvider
│
└── Local Runtime Domain                # V2
    ├── InstallationDetector
    ├── PiEnvironmentDoctor
    ├── LocalServiceProbe
    ├── ProcessSupervisor
    ├── LocalRuntimeSettingsStore
    └── RuntimeLogBuffer
```

核心原则：

1. **不把本机 Pi Hub 当成普通 Direct URL 配置。**
2. **Rust 是安装、诊断、进程和运行状态的唯一真实来源。**
3. **只停止当前 Desktop 实例明确持有 Handle 的进程。**
4. **不向前端开放通用 Shell、Spawn 或 Kill。**
5. **外部 `pi` CLI 不是 Pi Hub 启动的硬依赖。**
6. **Service WebView 继续保持零 Tauri Capability。**
7. **macOS 实现不能破坏 iOS 构建与 V1 功能。**

---

## 2. 现有架构接入点

当前仓库已经具备：

```text
src/
├── app/
├── features/
├── components/
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

V2 在不重写 V1 模块的前提下新增：

```text
src/
└── features/
    └── local-runtime/
        ├── api.ts
        ├── types.ts
        ├── LocalRuntimeCard.tsx
        ├── LocalRuntimeSettingsPage.tsx
        ├── EnvironmentDoctorPage.tsx
        ├── RuntimeLogsPage.tsx
        └── *.test.tsx

src-tauri/src/
├── commands/
│   └── local_runtime.rs
├── local_runtime/
│   ├── mod.rs
│   ├── model.rs
│   ├── manager.rs
│   ├── detector.rs
│   ├── doctor.rs
│   ├── health.rs
│   ├── process.rs
│   ├── settings.rs
│   ├── logs.rs
│   └── redaction.rs
└── platform/
    ├── macos/
    │   └── process.rs
    └── unsupported_local_runtime.rs
```

V2 不提前拆独立 crate。只有 ProcessSupervisor 或 Doctor 协议出现跨项目复用后再评估拆分。

---

## 3. 总体运行架构

```text
Trusted App Shell
        │
        │ typed Tauri Commands / Events
        ▼
LocalRuntimeManager
├── InstallationDetector
├── PiEnvironmentDoctor
├── LocalServiceProbe
├── ProcessSupervisor
├── SettingsStore
└── RuntimeLogBuffer
        │
        ├── absolute node executable
        ├── verified pi-hub entrypoint
        └── fixed arguments + controlled environment
                │
                ▼
        @jarome/pi-hub process
                │
                ▼
        127.0.0.1:30142
                │
                ▼
Untrusted Pi Hub Service WebView
```

`LocalRuntimeManager` 不进入 V1 的 `ConnectionManager`。两者可以共享 Viewer，但生命周期和状态模型保持独立。

---

## 4. 核心领域对象

### 4.1 LocalRuntimeManager

示意：

```rust
pub struct LocalRuntimeManager {
    state: tokio::sync::RwLock<LocalRuntimeSnapshot>,
    settings: Arc<LocalRuntimeSettingsStore>,
    detector: Arc<dyn InstallationDetector>,
    doctor: Arc<dyn PiEnvironmentDoctor>,
    probe: Arc<dyn LocalServiceProbe>,
    supervisor: Arc<dyn ProcessSupervisor>,
    logs: Arc<RuntimeLogBuffer>,
    operation_generation: AtomicU64,
}
```

职责：

- 统一序列化 start / stop / restart / scan / doctor 操作；
- 维护安装、环境和运行快照；
- 避免重复启动和旧异步任务覆盖新状态；
- 管理当前 App 实例持有的 Managed Process Handle；
- 向前端发送非敏感状态事件。

### 4.2 LocalRuntimeSnapshot

```rust
pub struct LocalRuntimeSnapshot {
    pub installation: InstallationState,
    pub runtime: LocalRuntimeState,
    pub environment: EnvironmentSummary,
    pub managed_process: Option<ManagedProcessSummary>,
    pub effective_url: Option<String>,
    pub last_error: Option<AppErrorDto>,
    pub checked_at: Option<DateTime<Utc>>,
}
```

快照只包含可序列化的展示信息，不包含：

- Child Handle；
- Secret；
- 完整环境变量；
- Pi 配置文件内容；
- stdout / stderr 无上限内容。

### 4.3 运行状态

```rust
pub enum LocalRuntimeState {
    Unknown,
    Checking,
    Stopped,
    Starting,
    RunningManaged,
    RunningExternal,
    Stopping,
    PortConflict,
    Failed,
}
```

主要合法转换：

```text
Unknown → Checking
Checking → Stopped | RunningManaged | RunningExternal | PortConflict | Failed
Stopped → Starting
Starting → RunningManaged | Stopped | PortConflict | Failed
RunningManaged → Stopping | Failed
Stopping → Stopped | Failed
RunningExternal → Checking
PortConflict → Checking
Failed → Checking | Starting
```

非法转换返回 typed error，不静默修改状态。

---

## 5. 安装模型

### 5.1 NodeInstallation

```rust
pub struct NodeInstallation {
    pub executable: PathBuf,
    pub canonical_executable: PathBuf,
    pub version: semver::Version,
    pub source: InstallationSource,
}
```

### 5.2 PiHubInstallation

```rust
pub struct PiHubInstallation {
    pub package_root: PathBuf,
    pub entrypoint: PathBuf,
    pub version: semver::Version,
    pub node_requirement: String,
    pub source: InstallationSource,
}
```

### 5.3 PiCliInstallation

```rust
pub struct PiCliInstallation {
    pub executable: PathBuf,
    pub version: Option<semver::Version>,
    pub kind: PiCliKind,
    pub source: InstallationSource,
}

pub enum PiCliKind {
    Npm,
    Standalone,
    Unknown,
}
```

该对象只用于信息检查，不参与 Pi Hub 启动命令构造。

### 5.4 安装来源

```rust
pub enum InstallationSource {
    Persisted,
    Path,
    Homebrew,
    Nvm,
    Volta,
    Fnm,
    Asdf,
    Mise,
    Manual,
}
```

---

## 6. 安装发现算法

### 6.1 总体顺序

```text
1. 验证用户已保存路径
2. 扫描当前 App PATH
3. 扫描固定系统目录
4. 扫描用户版本管理器目录
5. 对候选进行严格验证和去重
6. 自动选择最高优先级兼容组合
7. 无可用组合时提示手动选择
```

### 6.2 Node 候选路径

至少检查：

```text
/opt/homebrew/bin/node
/usr/local/bin/node
/usr/bin/node

~/.volta/bin/node
~/.nvm/versions/node/*/bin/node
~/.local/share/fnm/node-versions/*/installation/bin/node
~/.asdf/shims/node
~/.local/share/mise/shims/node
```

同时解析当前 App PATH 中的 `node`。

### 6.3 Pi Hub 候选路径

至少检查：

```text
/opt/homebrew/bin/pi-hub
/usr/local/bin/pi-hub

~/.volta/bin/pi-hub
~/.nvm/versions/node/*/bin/pi-hub
~/.local/share/fnm/node-versions/*/installation/bin/pi-hub
~/.asdf/shims/pi-hub
~/.local/share/mise/shims/pi-hub
```

### 6.4 候选验证

Pi Hub 候选不能只判断文件名。必须：

1. 解析 symlink 并 canonicalize；
2. 定位相邻或上级 `package.json`；
3. 验证 `name === "@jarome/pi-hub"`；
4. 验证 `bin.pi-hub` 指向候选入口；
5. 读取 package version 和 engines.node；
6. 验证 `.next` 生产构建存在；
7. 使用候选 Node 执行 `pi-hub --version --json`；
8. 设置短超时并校验返回 schema。

不允许通过执行任意用户脚本来“试试看”。

### 6.5 Node 与 Pi Hub 配对

优先配对规则：

1. Pi Hub bin 与 Node 位于同一版本管理器安装前缀；
2. 用户显式选择的配对；
3. 满足 `engines.node` 的最高优先级 Node；
4. 多个组合相同优先级时选择版本最高且路径最稳定者。

最终启动必须保存并使用绝对 `node` 与 `pi-hub.js` 路径，不能依赖 shebang 中的 `/usr/bin/env node`。

### 6.6 外部 Pi CLI 发现

检查路径与 Node/Pi Hub 类似，并执行：

```bash
/absolute/path/to/pi --version
```

规则：

- 超时后标记 informational failure；
- 不执行交互模式；
- 不执行 `pi auth ... --credentials`；
- 不参与本机 Pi Hub 启动阻断判断。

---

## 7. Local Runtime Settings

使用独立的版本化文件，例如：

```text
~/Library/Application Support/top.jiangliuhong.pihubclient/local-runtime.json
```

模型：

```rust
pub struct LocalRuntimeSettingsV1 {
    pub schema_version: u32,
    pub port: u16,
    pub auto_start_on_app_launch: bool,
    pub stop_managed_on_app_exit: bool,
    pub node_executable: Option<PathBuf>,
    pub pi_hub_entrypoint: Option<PathBuf>,
    pub pi_hub_package_root: Option<PathBuf>,
    pub pi_agent_dir: Option<PathBuf>,
    pub pi_hub_credential_id: Option<String>,
}
```

默认值：

```json
{
  "schema_version": 1,
  "port": 30142,
  "auto_start_on_app_launch": false,
  "stop_managed_on_app_exit": true
}
```

规则：

- 原子写入：temp file + fsync + rename；
- 路径保存前 canonicalize；
- Secret 只保存 Keychain credential id；
- 与 V1 Profiles 分开迁移；
- 设置变化使安装和 Doctor 缓存失效。

---

## 8. Pi Environment Doctor

### 8.1 设计原则

Doctor 必须是诊断器，不是修复器。

- 默认离线；
- 不调用模型 API；
- 不刷新 OAuth；
- 不输出 Secret；
- 每项检查有稳定 ID；
- 返回 machine-readable JSON；
- Desktop 不猜测 Pi Hub 内部模块结构。

### 8.2 Doctor 调用链

```text
LocalRuntimeManager
        ↓
PiEnvironmentDoctor
        ↓
verified node + pi-hub entrypoint
        ↓
pi-hub doctor --json --offline
        ↓
parse + schema validate + redact
        ↓
EnvironmentReport
```

Desktop 负责的检查：

- Node 可执行文件；
- Node version；
- Pi Hub 路径和包身份；
- 端口合法性；
- 用户选择路径安全性；
- 可选外部 Pi CLI。

Pi Hub Doctor 负责的检查：

- 内嵌 Pi Runtime 可加载；
- Pi Agent 目录；
- Session 目录；
- Settings / models 配置解析；
- Provider 认证是否至少存在可用候选；
- Model Runtime 基础状态。

### 8.3 Doctor JSON Schema

```json
{
  "schemaVersion": 1,
  "overall": "ready",
  "generatedAt": "2026-08-11T00:00:00Z",
  "piHub": {
    "name": "@jarome/pi-hub",
    "version": "0.0.x",
    "protocolVersion": 1,
    "embeddedPiVersion": "0.84.x"
  },
  "checks": [
    {
      "id": "DEP-PI-EMBEDDED-001",
      "category": "pi_environment",
      "severity": "required",
      "status": "pass",
      "code": "embedded_runtime_ready",
      "message": "Embedded Pi runtime is available",
      "remediation": null,
      "details": {
        "version": "0.84.x"
      }
    }
  ]
}
```

### 8.4 字段约束

`details` 采用检查级白名单，只允许：

- 版本；
- 脱敏路径；
- Provider 名称；
- 布尔状态；
- 非敏感错误码。

禁止：

- API Key；
- Authorization Header；
- OAuth Token；
- Cookie；
- auth.json 内容；
- models.json 全文；
- Session 内容；
- 完整环境变量。

### 8.5 Doctor 退出码

```text
0  ready
1  degraded
2  blocked
3  invalid invocation / internal error
```

Desktop 同时校验 JSON `overall` 和退出码；冲突时按内部错误处理。

### 8.6 检查聚合规则

```text
任一 required fail      → blocked
无 required fail，存在 recommended fail/warning → degraded
其他                    → ready
```

External Pi CLI 缺失属于 informational，不改变 overall。

---

## 9. Pi Hub Doctor 内部实现建议

该部分在 `jiangliuhong/pi-hub` 仓库实现。

推荐增加：

```text
bin/
├── pi-hub.js
└── commands/
    ├── version.js
    └── doctor.js

lib/
└── pi-environment-diagnostics.ts

app/api/client-info/
└── route.ts
```

### 9.1 共享诊断逻辑

将模型与 Pi Runtime 的非敏感检查提取到：

```ts
runPiEnvironmentDiagnostics(options)
```

命令行 Doctor 与未来设置页面可以共享，不复制一套判断。

### 9.2 离线约束

`--offline` 模式下：

- 设置 `PI_OFFLINE=1` 或使用等价 SDK 参数；
- 不刷新 Provider catalog；
- 不发送模型请求；
- 不刷新 OAuth；
- 只读取本地状态；
- 本地状态不足时返回 `skipped` 或 `warning`，不能偷偷联网。

### 9.3 认证检查

Doctor 只回答：

```text
是否存在至少一个本地可用认证候选
```

不返回凭据值。不得使用带 `--credentials` 的 Pi auth 命令。

### 9.4 Agent 目录

默认使用 Pi SDK 的 `getAgentDir()` 语义：

```text
PI_CODING_AGENT_DIR
或
~/.pi/agent
```

禁止 Desktop 和 Pi Hub 分别实现不一致的默认路径逻辑。

---

## 10. 服务身份与健康探测

### 10.1 `/api/client-info`

Pi Hub 提供：

```http
GET /api/client-info
```

响应：

```json
{
  "service": "pi-hub",
  "version": "0.0.x",
  "protocolVersion": 1
}
```

响应头：

```text
Cache-Control: no-store
Content-Type: application/json
```

### 10.2 认证策略

为了在不知道 Pi Hub Password 时也能区分端口冲突，该接口建议：

- 保留现有 Host / Request Security 校验；
- 只对 `/api/client-info` 豁免 HTTP Basic Auth；
- 不返回任何运行配置、会话、模型或用户数据；
- 只接受 GET；
- 保持 loopback / allowed host 限制。

Viewer 和其他 API 继续执行现有 Basic Auth。

### 10.3 探测分类

```rust
pub enum ProbeResult {
    NotListening,
    PiHub {
        version: Version,
        protocol_version: u32,
    },
    UnauthorizedPiHub,
    OtherService,
    TimedOut,
}
```

在采用 client-info 认证豁免后，`UnauthorizedPiHub` 仅作为兼容旧版本的保留状态。

### 10.4 协议兼容

Desktop 定义：

```rust
const SUPPORTED_CLIENT_PROTOCOL_MIN: u32 = 1;
const SUPPORTED_CLIENT_PROTOCOL_MAX: u32 = 1;
```

- 低于最小版本：提示升级 Pi Hub；
- 高于最大版本：允许用户查看说明，但默认不执行进程管理动作，避免错误解释协议；
- 版本匹配：正常运行。

---

## 11. ProcessSupervisor

### 11.1 接口

```rust
#[async_trait]
pub trait ProcessSupervisor: Send + Sync {
    async fn start(&self, spec: StartSpec) -> Result<ManagedProcess, ProcessError>;
    async fn stop(&self, process: &mut ManagedProcess) -> Result<StopResult, ProcessError>;
    async fn wait(&self, process: &mut ManagedProcess) -> Result<ExitSummary, ProcessError>;
}
```

### 11.2 StartSpec

```rust
pub struct StartSpec {
    pub node_executable: PathBuf,
    pub pi_hub_entrypoint: PathBuf,
    pub package_root: PathBuf,
    pub port: u16,
    pub pi_agent_dir: Option<PathBuf>,
    pub pi_hub_password: Option<Secret>,
}
```

`Secret` 不实现 `Debug`，Drop 时尽力清零。

### 11.3 启动命令

实际等价于：

```text
/absolute/path/to/node
/absolute/path/to/@jarome/pi-hub/bin/pi-hub.js
--hostname 127.0.0.1
--port 30142
--no-open
```

禁止：

```text
sh -c "..."
zsh -lc "..."
/usr/bin/env node ...
```

### 11.4 Working Directory

设置为验证后的 Pi Hub package root，避免相对资源解析不一致。

### 11.5 Environment

默认继承 App 进程环境，但：

- 将 Node 所在目录放到 PATH 首位；
- 强制 `PI_HUB_HOSTNAME=127.0.0.1`；
- 强制 `PI_HUB_NO_OPEN=1`；
- 如用户设置 Agent 目录，则注入 `PI_CODING_AGENT_DIR`；
- 如配置 Pi Hub Password，则从 Keychain 读取并仅注入子进程；
- 不读取或执行 `.zshrc`、`.bashrc`；
- 不允许用户配置任意环境变量；
- 日志不得打印环境。

说明：从 Finder 启动时不会自动获得用户交互式 Shell 中的 API Key。用户应优先使用 Pi `/login` 等持久化认证；Doctor 对缺失认证给出 degraded 提示。

### 11.6 stdout / stderr

- 使用 pipe 捕获；
- 两路独立异步读取；
- 进入固定大小 ring buffer；
- 单行长度设上限；
- 进入 UI 前执行 redaction；
- 不把完整输出作为 Tauri event 高频广播。

### 11.7 进程组

macOS 启动时创建独立进程组：

- 优先使用 `std::os::unix::process::CommandExt::process_group(0)`；
- 平台信号实现集中在 `platform/macos/process.rs`；
- 优雅退出向受管进程组发送 SIGTERM；
- 超时后向同一受管进程组发送 SIGKILL；
- 不向系统其他同名进程广播信号。

如标准库能力不足，可在该窄平台模块增加 `nix` 或 `libc`，不得把 unsafe signal 逻辑散落在业务层。

### 11.8 进程所有权

所有权只由内存中的 `ManagedProcess` Handle 证明。

- PID 只用于诊断；
- 不把 PID 文件当成所有权证明；
- App 崩溃或重启后，旧服务视为 external；
- 不从端口反查 PID 后取得管理权。

---

## 12. 启动流程

```text
start_local_pi_hub
        ↓
获取操作锁和 generation
        ↓
刷新 LocalServiceProbe
        ├── Pi Hub 已运行 → RunningExternal / RunningManaged，返回
        ├── 其他服务占用 → PortConflict，返回
        └── 未监听 → 继续
        ↓
验证 Installation
        ↓
运行或刷新 required Doctor checks
        ├── blocked → 返回
        └── ready/degraded → 继续
        ↓
从 Keychain 读取可选 Pi Hub Password
        ↓
ProcessSupervisor.start
        ↓
状态 Starting
        ↓
轮询 /api/client-info
        ├── Ready → RunningManaged
        ├── Child 提前退出 → Failed
        ├── 端口变成其他服务 → PortConflict + 停止受管进程
        └── 30 秒超时 → 停止受管进程 + Failed
```

### 12.1 Ready 策略

- 初始间隔 200ms；
- 最大间隔 1s；
- 总超时 30s；
- 不解析 stdout 中的 `Ready` 字符串；
- 成功必须以 `/api/client-info` 为准。

### 12.2 并发保护

- 同一时间只允许一个 start / stop / restart；
- 重复点击返回当前操作，不生成第二个进程；
- 每次操作持有 generation；
- 旧探测任务完成时若 generation 已变化，丢弃结果。

---

## 13. 停止与重启流程

### 13.1 停止

```text
stop_local_pi_hub
        ↓
确认 state == RunningManaged
        ↓
取出 ManagedProcess Handle
        ↓
状态 Stopping
        ↓
SIGTERM 进程组
        ↓
最多等待 5 秒
        ├── 已退出 → 检查端口释放
        └── 未退出 → SIGKILL 进程组
        ↓
清理 reader task / handle / runtime metadata
        ↓
状态 Stopped 或 Failed
```

### 13.2 外部进程

`RunningExternal` 调用 stop 返回：

```text
external_process_not_owned
```

前端显示说明，不提供自动获取权限或强制 Kill。

### 13.3 重启

```text
RunningManaged → stop → required checks → start
```

如果停止后端口仍未释放，返回 `port_not_released`，不启动第二实例。

---

## 14. 自动启动与崩溃循环保护

> **设计决策（手动检测模型）**：当前版本未在 App 启动时调用 `initialize_local_runtime`，窗口聚焦时也不再自动 refresh。本机 Runtime 检测与启停完全由用户在「This Mac」卡片手动触发（`scan_local_installations` / `start` / `stop` / `restart`）。`LocalRuntimeManager::initialize()` 方法与崩溃循环保护代码保留，供未来恢复自动启动时复用。详见 `docs/requirements-v2.md` §3.2。

### 14.1 App 启动

Trusted App Shell 创建后异步调用：

```text
initialize_local_runtime
```

流程：

1. 加载设置；
2. 发布 `checking`；
3. 扫描安装和探测状态；
4. 自动启动关闭时停止；
5. 自动启动开启且状态 stopped 时执行 start；
6. 失败只更新本机卡片和通知，不阻塞远程服务列表。

### 14.2 崩溃循环保护

持久化最近自动启动失败时间，不保存敏感日志。

建议规则：

```text
5 分钟内自动启动失败 >= 3 次
→ auto_start_suppressed
```

恢复条件：

- 用户手动点击“启动”；
- 用户修改安装、端口或 Agent 目录设置；
- 距最近失败超过保护窗口。

手动启动失败仍返回错误，但不会被静默吞掉。

### 14.3 App 退出

在 Tauri `ExitRequested` 阶段：

- 如果设置为 stop managed on exit 且存在 Managed Process：
  - 临时阻止立即退出；
  - 执行有限时长的异步 stop；
  - 完成或超时后继续退出；
- 外部 Pi Hub 不处理；
- 关闭窗口但 App 尚未退出时不停止。

必须防止退出回调重复触发。

---

## 15. 日志设计

### 15.1 内存 Ring Buffer

```rust
pub struct RuntimeLogBuffer {
    lines: VecDeque<RedactedLogLine>,
    max_lines: usize,
    max_line_bytes: usize,
}
```

建议：

- 最多 500 行；
- 单行最多 16 KiB；
- UI 默认展示最近 200 行；
- stdout / stderr 标明来源；
- 时间戳由 Desktop 增加。

### 15.2 磁盘日志

可选保存最近一次启动：

```text
~/Library/Logs/Pi Hub Client/local-pi-hub.log
```

- 最大 1 MiB；
- 只保留当前和上一份；
- 原子轮换；
- 应用退出时 flush；
- 不记录 Pi Hub HTTP 请求和对话数据。

### 15.3 Redaction

至少覆盖：

- `Authorization:`；
- `Bearer ...`；
- 常见 `*_API_KEY=`；
- `PI_HUB_PASSWORD=`；
- Telegram Bot Token 形式；
- OpenSSH Private Key header；
- Cookie / Set-Cookie。

Redaction 是最后一道保护，不允许先主动打印 Secret 再依赖正则擦除。

---

## 16. Tauri Commands

```rust
get_local_runtime_status()
scan_local_installations()
validate_local_installation(input)
run_local_environment_doctor(force: bool)
start_local_pi_hub()
stop_local_pi_hub()
restart_local_pi_hub()
get_local_runtime_settings()
update_local_runtime_settings(input)
get_local_runtime_logs(limit: Option<u32>)
clear_local_runtime_logs()
```

规则：

- Command 保持薄；
- 所有业务在 `LocalRuntimeManager`；
- 前端不能传任意 command / args / pid；
- `start` 使用已保存且验证后的安装，不接受一次性任意路径；
- Error 转换为稳定 `AppErrorDto`。

### 16.1 平台门控

所有命令在 iOS 可编译，但返回稳定错误：

```text
unsupported_platform
```

前端通过平台能力隐藏入口。不要复制一套 iOS Command 实现。

### 16.2 Capability

命令只属于现有 Trusted `main` window capability。

- 不增加 remote URL capability；
- 不允许 Service WebView 调用；
- 不启用通用 shell plugin permission；
- 不为实现进程启动扩大远端权限。

---

## 17. 前端状态与事件

### 17.1 API 边界

`src/features/local-runtime/api.ts` 只封装 typed invoke，不保存进程真相。

### 17.2 事件

Rust 可以发送低频事件：

```text
local-runtime://status-changed
local-runtime://doctor-completed
local-runtime://logs-updated
```

Event payload 只包含 DTO 或新日志计数，不包含 Secret 和完整日志流。

### 17.3 页面状态

React 使用服务器快照：

- 初次加载调用 `get_local_runtime_status`；
- 收到 status event 后重新同步；
- 按钮状态由 enum 决定；
- 不基于本地 optimistic state 假装进程已经启动或停止。

---

## 18. Error Codes

新增至少：

```text
local_runtime_unsupported_platform
local_runtime_operation_in_progress
node_not_found
node_version_incompatible
node_execution_failed
pi_hub_not_found
pi_hub_installation_invalid
pi_hub_version_incompatible
pi_hub_doctor_invalid_output
pi_hub_doctor_blocked
pi_agent_dir_unavailable
pi_session_dir_unavailable
pi_auth_not_configured
pi_model_not_available
local_port_conflict
local_service_probe_timeout
local_service_protocol_incompatible
local_process_start_failed
local_process_exited_early
local_process_not_owned
local_process_stop_timeout
local_port_not_released
auto_start_suppressed
local_runtime_cancelled
```

错误 `details` 使用白名单字段：

```text
port
stage
version
requiredVersion
path（允许时）
exitCode
signal
checkId
retryCount
```

不包含环境和日志全文。

---

## 19. 新依赖评估

实现前优先复用现有依赖。若新增，建议：

### `semver`

用途：

- 解析 Node、Pi Hub、Pi CLI 版本；
- 校验 npm engines 风格的简单 VersionReq。

### `reqwest` 或等价最小 HTTP Client

用途：

- loopback `/api/client-info`；
- 明确 timeout；
- JSON schema 解析。

要求支持 macOS 与 iOS；不得关闭 TLS 校验。若现有项目已有合适 HTTP Client，禁止重复引入。

### `nix` / `libc`（仅必要时）

用途：

- macOS 受管进程组的 SIGTERM / SIGKILL。

信号逻辑必须封装在单一平台模块。

每个新增依赖仍需遵循 AGENTS.md 的兼容性、lockfile 和安全评审规则。

---

## 20. Pi Hub CLI 信号处理

当前 Pi Hub CLI 启动 Next.js 子进程。V2 需要在 Pi Hub 仓库中确保：

```text
SIGTERM / SIGINT
        ↓
pi-hub outer CLI
        ↓
Next.js child
        ↓
等待 child exit
        ↓
outer CLI 使用相同退出语义退出
```

要求：

- 信号处理幂等；
- 第二次信号可以加速退出；
- 设置强制退出上限；
- 不在正常停止时打印误导性 crash error；
- 有自动化测试验证 child 不残留。

Desktop 仍终止受管进程组，Pi Hub 的信号转发用于完成应用级清理。

---

## 21. 测试设计

### 21.1 Rust 单元测试

- known path discovery；
- NVM / Volta / FNM 路径识别；
- symlink canonicalize；
- package identity validation；
- Node VersionReq；
- Doctor JSON parsing 和 schema version；
- overall aggregation；
- settings migration；
- state machine；
- crash-loop protection；
- log redaction；
- Secret 不实现 Debug。

### 21.2 Rust 集成测试

通过可注入接口构造：

- fake installation tree；
- helper child process；
- mock `/api/client-info` server；
- blocked / degraded / ready Doctor output；
- early exit；
- startup timeout；
- graceful stop；
- forced stop；
- port conflict；
- external process refusal；
- duplicate start；
- stale generation result discarded。

测试不得 Kill 测试进程之外的 PID。

### 21.3 Pi Hub 仓库测试

- `--version --json` schema；
- `doctor --json --offline` 各退出码；
- Doctor 输出不包含 mock Secret；
- `/api/client-info` Host Security；
- client-info 不泄露配置；
- Basic Auth 开启时 client-info 仍可按设计识别；
- SIGTERM 后 Next child 无残留。

### 21.4 前端测试

- macOS 显示 This Mac，iOS 隐藏；
- 每个 runtime state 的按钮矩阵；
- Doctor 分类和 remediation；
- blocked 禁止启动；
- degraded 允许启动并提示；
- external 不显示停止；
- port conflict；
- auto-start 设置；
- 日志页面不渲染未脱敏内容。

### 21.5 macOS 真机矩阵

至少覆盖：

| 场景 | 要求 |
|---|---|
| Homebrew Node + npm Pi Hub | 必测 |
| NVM | 必测 |
| Volta 或 FNM | 至少一项必测 |
| Finder 启动 App | 必测 |
| 终端启动外部 Pi Hub | 必测 |
| 30142 非 Pi Hub 占用 | 必测 |
| Node 版本过低 | 必测 |
| Agent 目录不可写 | 必测 |
| 无认证 / 无模型 | 必测 |
| `Cmd + Q` | 必测 |
| Developer ID 签名、公证后 App | 发布前必测 |

### 21.6 iOS 回归

- `npm run tauri ios build`；
- Direct URL；
- SSH Forward；
- Host Key；
- Viewer；
- 本机 Runtime 的“连接”复用受限 Viewer：进入 Viewer 前由 Rust `refresh`
  重新验证 `/api/client-info`，只加载其返回的 loopback `effective_url`，不调用
  系统浏览器，也不向被嵌入页面授予 Tauri capability；
- Keychain；
- 本机 Runtime UI 不可见。

---

## 22. 开发顺序

### Step 1：Pi Hub 契约

先在 `pi-hub` 完成：

```text
--version --json
doctor --json --offline
/api/client-info
signal forwarding
```

没有稳定契约前，不允许 Desktop 通过解析 stdout、扫描 node_modules 或猜测端口内容实现临时方案。

### Step 2：只读 Local Runtime

实现：

- Settings；
- Detector；
- Doctor parser；
- ServiceProbe；
- 状态卡片；
- 不启动进程。

### Step 3：Managed Process

实现：

- ProcessSupervisor；
- start / stop / restart；
- logs；
- state machine；
- ownership boundary。

### Step 4：自动启动和退出

实现：

- async initialization；
- crash-loop protection；
- stop-on-exit；
- focus refresh。

### Step 5：真机和分发

完成安装矩阵、签名、公证、DMG 测试，并记录 iOS 回归。

---

## 23. 关键设计决策摘要

| 决策 | 结论 |
|---|---|
| 本机 Pi Hub 是否保存成 Direct URL Profile | 否，使用独立 Local Runtime Domain |
| 外部 `pi` CLI 是否硬依赖 | 否，仅 informational |
| Pi Hub 如何判断 Pi Runtime | 通过 `pi-hub doctor`，Desktop 不扫描内部依赖 |
| 如何判断端口上的服务 | `/api/client-info`，不是只做 TCP connect |
| 是否解析 stdout 中的 Ready | 否 |
| 如何启动 | 绝对 Node + 绝对入口 + 固定参数，不使用 Shell |
| 如何证明进程所有权 | 当前 App 内存中的 Child Handle |
| 能否停止终端启动的 Pi Hub | 默认不能 |
| 自动启动是否等于开机启动 | 否 |
| App 退出后是否长期保留服务 | V2 默认不保留 |
| Service WebView 是否获得 Runtime Commands | 绝不 |
| iOS 是否实现本机进程管理 | 否，但必须保持编译与 V1 回归 |

---

## 24. Definition of Done

技术实现只有在以下条件全部满足时才完成：

- Desktop 与 Pi Hub 的 version / doctor / client-info 协议稳定并有 schema version；
- Detector 不依赖交互式 Shell；
- NVM 和 Homebrew 场景在 Finder 启动的签名 App 中通过；
- Doctor 离线、无费用、无 OAuth 刷新、无 Secret 输出；
- 外部 Pi CLI 缺失不会阻止 Pi Hub；
- 只有 Managed Process 可以停止；
- start / stop / restart 的并发、超时和资源释放有测试；
- 端口冲突不会误 Kill；
- 自动启动有崩溃循环保护；
- 日志已脱敏且有大小限制；
- Service WebView 保持零权限；
- iOS 构建与 V1 功能没有回归；
- 真机验证平台、命令和未验证项均被如实记录。
