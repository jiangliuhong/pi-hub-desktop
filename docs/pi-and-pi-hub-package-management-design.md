# Pi Hub Client：Pi 与 Pi Hub 检测、安装和更新设计

> 文档状态：Baseline（治理已对齐，Phase 0 完成）  
> 目标平台：macOS 14+  
> 关联版本：V3 Package Management（见 `docs/requirements-v3.md`）  
> 最后更新：2026-08-11

## 1. 文档目的

本文档定义 Pi Hub Client 在“设置”中管理本机 **Pi** 与 **Pi Hub** 的产品行为和技术实现，覆盖：

- 分别检测 Pi 与 Pi Hub 是否安装；
- 显示当前安装版本、安装来源和路径；
- 联网检查可用更新；
- 对已安装产品执行安全更新；
- 在任一产品未安装时执行初次安装；
- 在两者均未安装时分别安装，或通过“一键安装全部”完成首次准备；
- 安装、更新失败后的清理、恢复和可诊断错误；
- 与现有 Local Runtime 启停、Doctor、外部进程识别和 iOS 平台门控集成。

本文档是 V3 Package Management 的技术设计。Phase 0（治理对齐）已完成：已把本能力登记为 V3（`docs/requirements-v3.md`、`README.md`、`AGENTS.md` §1/§9），并同步修订了 `AGENTS.md` §2.3 与 `requirements-v2.md` §5.2 中“不自动安装/升级”的范围冲突。进入功能代码实现前仍需关闭 §22 Phase 0 / §24 中两项跨仓库确认（两个发布包是否支持 `--ignore-scripts`、版本命令稳定 schema）；这些确认需在 `jiangliuhong/pi-hub` 与 `@earendil-works/pi-coding-agent` 仓库完成并记录结论，不得在 Desktop 侧用临时猜测逻辑绕过。

## 2. 背景与当前问题

### 2.1 当前实现

`pi-hub-desktop` 已存在完整的 V2 本机 Runtime 基础：

- `DefaultInstallationDetector` 扫描 App PATH、Homebrew、NVM、Volta、FNM、ASDF、Mise 和用户保存路径；
- `InstallationSet` 返回 Node.js、Pi Hub 和可选外部 Pi CLI；
- `LocalRuntimeManager` 维护安装、Doctor、端口探测和进程状态；
- `LocalRuntimeSettingsPage` 支持手动填写路径、重新扫描和环境检查；
- `LocalRuntimeCard` 支持启动、停止、重启和打开 Pi Hub；
- Rust 是安装事实、运行状态和受管子进程的唯一真实来源；
- iOS 隐藏本机 Runtime 功能。

### 2.2 现有缺口

1. 外部 Pi CLI 只属于 informational check，不是可管理产品。
2. 检测结果使用 `Option` 表达，无法完整区分“未安装、损坏、版本不兼容、暂时无法执行”。
3. 没有远端版本元数据，不知道是否存在更新。
4. 没有安装器、进度、取消、事务、回滚或安装日志模型。
5. 没有区分 Desktop 受管安装与用户外部安装的更新权限。
6. 设置弹窗目前只有“外观”；Local Runtime 设置在独立页面。
7. Pi Hub 更新期间可能已有受管或外部服务运行，必须遵守 `ProcessSupervisor` 的所有权规则。
8. Finder 启动的 App 不能假设存在交互式 Shell PATH，也不能通过 `zsh -c` 执行全局 npm 命令。

## 3. 术语与产品定义

### 3.1 Pi

本文中的 **Pi** 指外部命令行产品：

```text
package: @earendil-works/pi-coding-agent
binary:  pi
```

Pi Hub 安装包内部也依赖 Pi Runtime，但两者不是同一安装对象：

- 更新外部 Pi，不改变 Pi Hub 内嵌 Pi Runtime；
- 更新 Pi Hub，可能随 Pi Hub 包版本一起更新内嵌 Pi Runtime；
- 外部 Pi 未安装不应阻止 Pi Hub 启动；
- 设置页必须明确展示这种关系，避免用户误以为两个版本必须一致。

### 3.2 Pi Hub

本文中的 **Pi Hub** 指本机 Web 服务产品：

```text
package: @jarome/pi-hub
binary:  pi-hub
```

Pi Hub 仍由现有 `LocalRuntimeManager` 启动和管理；新包管理模块只负责获取、安装、验证和激活软件包，不复制 Runtime 进程管理逻辑。

### 3.3 Node.js 和 npm

Pi 与 Pi Hub 都需要兼容 Node.js。第一阶段不自动安装或更新 Node.js，Node.js 仍是安装动作的前置条件。

“Pi 与 Pi Hub 均未安装时可以初次安装”的准确含义：

- 若存在兼容 Node.js 和可验证 npm，用户可以分别安装两者或一键安装全部；
- 若 Node.js 缺失或版本不兼容，安装按钮显示阻断原因和修复建议；
- 若 Node.js 可用但 npm 不可用，检测仍可完成，但自动安装被阻断；
- “零依赖首次安装”需要另行设计受管 Node Runtime，不在本阶段隐式下载可执行 Runtime。

## 4. 目标与非目标

### 4.1 目标

- 检测准确、可解释，不能把“无法验证”显示为“未安装”。
- Pi 和 Pi Hub 独立管理；一个失败不应破坏另一个。
- 默认不修改用户的 Homebrew、NVM、Volta、FNM、ASDF 或 Mise 安装。
- 新安装和更新位于当前用户 Application Support 目录，不需要 `sudo`。
- 使用固定包名、固定参数和精确版本，禁止任意 Shell。
- 更新先安装到 staging，再验证并原子激活；失败保留旧版本。
- Pi Hub 更新必须尊重 managed/external 进程所有权。
- 离线时仍展示本机状态，只把更新状态标记为未知。
- Service WebView 不能调用安装、更新或版本查询命令。
- iOS 继续编译并保留 V1 功能，不显示本机包管理入口。

### 4.2 非目标

- 自动安装或升级 Node.js；
- 自动更新 Desktop App 自身；
- 卸载或覆盖用户的外部全局安装；
- 执行 `sudo` 或写入 `/usr/local`、`/opt/homebrew`；
- 修改 Shell 配置或 PATH；
- 承诺 Desktop 受管 Pi 自动出现在终端 PATH；
- 支持任意 npm package、registry URL 或安装参数；
- 在 iOS 上管理本机 Pi/Pi Hub；
- 后台静默更新；
- 强制停止不是当前 Desktop 实例启动的 Pi Hub；
- 引入通用 Shell、Terminal、PTY 或 Process Tauri plugin 权限。

## 5. 核心设计决策

### 5.1 采用“受管副本”，不原地修改外部安装

Desktop 自动安装或更新时，目标始终是 Pi Hub Client 自己的用户级受管目录。即使检测到 Homebrew、NVM 或 Volta 安装，默认更新动作也是：

```text
安装新的 Desktop 受管副本 -> 验证 -> 请求用户切换使用
```

不直接执行 `npm update -g`、`brew upgrade`、`volta install`，也不修改 NVM 当前版本的全局包。原因：

- 外部目录可能不可写或需要管理员权限；
- Desktop 无法可靠还原用户包管理器状态；
- Finder 启动时版本管理器的“当前版本”不稳定；
- 原地失败可能破坏当前可用版本；
- 受管副本可原子切换和回滚。

设置页仍展示外部安装。按钮根据来源显示“安装受管版本”或“更新受管版本”，不能暗示外部环境已被修改。

### 5.2 Pi 与 Pi Hub 隔离安装

```text
~/Library/Application Support/Pi Hub Client/
└── packages/
    ├── manifest.json
    ├── pi/
    │   ├── versions/<version>/node_modules/...
    │   ├── staging/
    │   └── logs/
    └── pi-hub/
        ├── versions/<version>/node_modules/...
        ├── staging/
        └── logs/
```

`manifest.json` 保存 active version、安装时间、Node 绝对路径、包名、入口和来源，不保存凭据。两个产品隔离，避免 npm 对共享 `node_modules` 的重排互相影响。

### 5.3 不自动后台更新

App 可按缓存策略自动检查更新，但安装和切换必须由用户显式点击。Pi Hub 更新可能需要重启，本阶段不做静默更新。

## 6. 用户体验设计

### 6.1 设置结构

现有设置弹窗调整为分区或侧栏：

```text
设置
├── 通用
│   └── 外观
└── 本机组件                  macOS only
    ├── Node.js 前置条件
    ├── Pi
    └── Pi Hub
```

原有 `/local-runtime/settings` 保留为高级设置页，继续管理路径、端口、自动启动和退出行为；“设置 -> 本机组件”作为普通用户主入口。

### 6.2 组件卡片

每个产品卡片至少显示：

```text
Pi
用于终端交互；不影响 Pi Hub 内嵌 Runtime

状态：已安装 / 未安装 / 有更新 / 安装损坏 / 不兼容
当前版本：0.84.0
最新版本：0.85.0
来源：Desktop 受管 / Homebrew / NVM / Volta / 手动路径
位置：~/Library/.../pi（可展开）

[检查更新] [更新]
```

Pi Hub 卡片额外显示 Runtime 状态、是否由当前 Desktop 管理、更新是否需要重启，以及当前使用安装是否不同于其他检测结果。

### 6.3 状态与主操作

| 本机状态 | 远端状态 | 主操作 | 说明 |
|---|---|---|---|
| 未安装 | 已知 | 安装 | 安装最新 stable 精确版本 |
| 未安装 | 未知/离线 | 重试检查 | 不猜测版本 |
| 已安装且有效 | 已是最新 | 检查更新 | 显示最后检查时间 |
| 已安装且有效 | 有更新 | 更新 | 安装受管新版本并验证 |
| 外部安装有效 | 有更新 | 安装受管新版 | 不覆盖外部环境 |
| 安装损坏 | 任意 | 修复安装 | 新建受管副本 |
| 版本不兼容 | 有兼容版本 | 安装兼容版本 | 说明 Node 兼容关系 |
| Node/npm 阻断 | 任意 | 不可操作 | 展示前置条件 |
| 正在操作 | 任意 | 取消 | 禁止重复操作 |
| 检查失败 | 未知 | 重试 | 保留本机安装事实 |

### 6.4 两者均未安装

当两者均为 `not_installed` 且前置条件满足时显示“一键安装 Pi 与 Pi Hub”。它是两个独立事务的串行编排：

1. 获取一次版本元数据；
2. 安装并验证 Pi；
3. 安装并验证 Pi Hub；
4. 各自成功后立即提交各自 manifest；
5. 第二项失败时不回滚已成功的第一项，显示“部分完成”；
6. 不默认启动 Pi Hub，完成后询问“现在启动”。

用户也可以分别点击两个卡片上的“安装”。

### 6.5 进度

显示阶段：

```text
准备中 -> 获取版本信息 -> 下载并安装 -> 验证 -> 切换版本 -> 完成
```

展示产品、操作类型、阶段、已耗时、取消按钮、脱敏错误和日志入口。npm 没有稳定的字节级进度契约，第一阶段不伪造百分比。

### 6.6 Pi Hub 运行时更新

| Runtime 状态 | 行为 |
|---|---|
| stopped/failed | 安装、验证并立即激活 |
| running_managed | 先下载验证；激活前提示“更新并重启” |
| running_external | 可下载受管新版，但不得停止外部进程；等待用户稍后激活 |
| starting/stopping/checking | 禁止更新，等待当前操作结束 |
| port_conflict | 可下载，但不得启动或宣称切换成功 |

`running_managed` 流程：

1. staging 安装和验证不影响当前服务；
2. 用户确认后调用现有 `LocalRuntimeManager.stop()`；
3. 原子切换 active manifest 和 Local Runtime 路径；
4. 调用 `LocalRuntimeManager.start()`；
5. `/api/client-info` 与 Doctor 验证通过后完成；
6. 若新版本启动失败，停止新进程、恢复旧 manifest/路径，并尝试恢复旧版本；
7. 明确显示回滚成功或回滚失败。

## 7. 总体架构

```text
Trusted Settings UI
        |
        | typed Tauri commands / events
        v
PackageManagementManager ------------------------------+
  |            |              |             |          |
  v            v              v             v          v
Detector   ReleaseClient  PackageInstaller  Verifier  ManagedPackageStore
  |            |              |             |          |
  |       HTTPS registry      |        identity/version| manifest + versions
  |                           |
  |                    absolute Node + npm-cli.js
  |
  +---------------- existing local install discovery

PackageManagementManager
        |
        +---- Pi Hub activation ----> existing LocalRuntimeManager
                                      -> Doctor
                                      -> ProcessSupervisor
                                      -> LocalServiceProbe
```

### 7.1 Rust 模块

```text
src-tauri/src/package_management/
├── mod.rs
├── manager.rs
├── model.rs
├── release_client.rs
├── npm_toolchain.rs
├── installer.rs
├── managed_store.rs
├── verifier.rs
└── operation.rs
```

- `InstallationDetector`：回答“本机有什么”，复用当前候选验证；
- `ReleaseClient`：回答 stable 最新版本；
- `NpmToolchainDetector`：找到与 Node 配套的 npm CLI；
- `PackageInstaller`：固定包名/参数的 staging 安装；
- `PostInstallVerifier`：验证身份、版本、bin、engine 和生产构建；
- `ManagedPackageStore`：manifest、journal、原子激活和旧版本清理；
- `PackageManagementManager`：状态聚合、并发、取消、事件和事务；
- `LocalRuntimeManager`：继续独占 Pi Hub 启停，不吸收 npm 逻辑。

### 7.2 前端模块

```text
src/features/package-management/
├── PackageManagementSettings.tsx
├── ProductCard.tsx
├── InstallProgress.tsx
├── UpdateConfirmationDialog.tsx
├── api.ts
├── labels.ts
└── types.ts
```

`SettingsPage.tsx` 只组合设置导航，不直接实现包管理状态机。

## 8. 检测模型

### 8.1 独立产品结果

```rust
enum ProductId { Pi, PiHub }

enum ProductInstallState {
    Unknown,
    NotInstalled,
    Installed,
    Invalid,
    Incompatible,
}

enum InstallOwnership { DesktopManaged, External }

struct ProductInstallation {
    product: ProductId,
    package_name: String,
    version: Option<String>,
    executable: Option<PathBuf>,
    package_root: Option<PathBuf>,
    source: InstallationSource,
    ownership: InstallOwnership,
    state: ProductInstallState,
    issue_code: Option<String>,
}
```

旧 `InstallationSet` 保留给 V2 Runtime，新快照通过 adapter 从 detector facts 构建，避免重写 `LocalRuntimeManager`。

### 8.2 当前安装选择顺序

1. 用户显式选择且重新验证通过；
2. Desktop active manifest 指向的受管安装；
3. V2 persisted path；
4. App PATH；
5. Homebrew、Volta、NVM、FNM、ASDF、Mise 等候选。

展示层保留所有有效候选。对 Pi Hub，`/api/client-info` 只证明服务身份，不反推出进程或磁盘安装所有权。

### 8.3 Pi 检测加强

当前 Pi 检测只执行 `pi --version`，需要增加：

1. canonicalize executable；
2. 定位 package root；
3. npm 类型验证 `package.json.name === "@earendil-works/pi-coding-agent"`；
4. 验证 `bin.pi`；
5. 解析严格 semver；
6. 验证 Node engine；
7. 区分 npm、standalone、Desktop managed 和 unknown；
8. standalone 只检测；更新时安装受管 npm 副本，不覆盖 binary。

### 8.4 检测时机

- App 启动只执行本机快速扫描，不阻塞窗口；
- 打开设置立即返回缓存并触发刷新；
- “重新扫描”强制本机扫描；
- 安装/更新成功后强制扫描与 Doctor；
- App 获得焦点沿用 V2 刷新；
- 网络版本查询独立，失败不得清空本机结果。

## 9. 远端版本检查

### 9.1 固定数据源

```text
Pi     -> @earendil-works/pi-coding-agent -> dist-tag latest
Pi Hub -> @jarome/pi-hub                   -> dist-tag latest
```

`ReleaseClient` 通过 HTTPS 访问固定 npm registry API，只解析 `dist-tags.latest`、对应 `version`、`engines.node`、`dist.integrity` 和可选发布时间。不得把 registry 原文传给前端或日志。

### 9.2 缓存

- 成功 TTL 6 小时；
- 用户手动检查忽略 TTL，但使用 ETag；
- 失败不覆盖最后一次成功结果；
- 失败后最短重试间隔 30 秒；
- 离线时 `update_status = unknown` 并显示最后成功时间。

### 9.3 版本策略

- 只使用 `latest` stable dist-tag；
- 不自动选择 prerelease；
- semver 比较，禁止字符串比较；
- 当前版本高于 latest 时显示 `newer_than_latest`，不得降级；
- 前端提交后端生成的短期 opaque release token，不能提交任意版本/package spec。

## 10. npm 工具链与固定命令

```rust
struct NpmToolchain {
    node_executable: PathBuf,
    npm_cli_js: PathBuf,
    npm_version: String,
    source: InstallationSource,
}
```

验证要求：

- Node 通过现有版本基线；
- npm CLI canonicalize；
- 使用绝对 Node 执行绝对 `npm-cli.js --version`；
- 短超时、stdin null、输出有界；
- 不执行 Shell 配置，不用 `which`/`command -v`；
- npm 与 Node 尽量来自同一前缀。

安装命令逻辑等价于：

```text
<absolute-node> <absolute-npm-cli.js>
install
--prefix <validated-staging-dir>
--no-save
--package-lock=false
--ignore-scripts
--no-audit
--no-fund
--omit=dev
<allowlisted-package>@<exact-version>
```

Rust 逐项构造参数。包名来自 `ProductId` 固定映射，版本来自 release token，staging path 由后端生成。禁止前端传 registry、package、命令、参数、环境或目标目录。默认 `--ignore-scripts`；若发布物不能支持，必须先改变发布方式或提交独立安全设计。

## 11. 安装和更新事务

### 11.1 初次安装

```text
校验平台/operation lock
-> 刷新本机状态
-> 校验 Node/npm
-> 获取并冻结 stable release
-> 创建 staging
-> 固定 npm install
-> 严格后置验证
-> rename 至 versions/<version>
-> 原子写 active manifest
-> 刷新 detector/Doctor
-> 返回完成快照
```

后置验证失败时删除 staging，active manifest 不变化。

### 11.2 更新

与初次安装共用事务，并增加：

- 记录 `previous_active_version`；
- 新版本验证前不改当前路径；
- Pi Hub 运行时按第 6.6 节协调；
- 激活后再次执行版本命令；
- Pi Hub 执行 Doctor 和 service identity 检查；
- 失败恢复旧 manifest 和 Runtime 路径。

### 11.3 原子提交

同文件系统写临时 manifest，`fsync` 后 rename；版本目录由 staging rename。禁止在 active 目录原地 npm install、先删除旧版本、让 detector 选中半成品或只用 symlink 表达事务。

### 11.4 清理

- 每产品保留 active 和最近一个成功版本；
- 失败 staging 立即尽力清理；
- 启动时清理超过 24 小时且未被 journal 引用的 staging；
- 不删除 active/rollback 引用；
- 清理失败不把可用安装标记为损坏。

## 12. 状态模型

```rust
enum UpdateStatus {
    Unknown, Checking, UpToDate, Available, NewerThanLatest, Unavailable,
}

enum PackageOperationKind { Install, Update, Repair, Activate }

enum PackageOperationStage {
    Preparing,
    FetchingMetadata,
    Installing,
    Verifying,
    AwaitingRestartConfirmation,
    Activating,
    Restarting,
    RollingBack,
    Completed,
    Cancelled,
    Failed,
}
```

前端快照：

```ts
interface PackageManagementSnapshot {
  platform_supported: boolean;
  prerequisites: {
    node: ProductPrerequisite;
    npm: ProductPrerequisite;
  };
  products: ProductStatus[];
  active_operation?: PackageOperation;
  checked_at?: string;
}

interface ProductStatus {
  product: "pi" | "pi_hub";
  install_state:
    | "unknown"
    | "not_installed"
    | "installed"
    | "invalid"
    | "incompatible";
  current?: ProductInstallationDto;
  alternatives: ProductInstallationDto[];
  update_status:
    | "unknown"
    | "checking"
    | "up_to_date"
    | "available"
    | "newer_than_latest"
    | "unavailable";
  latest_version?: string;
  last_update_check_at?: string;
  allowed_actions: ProductAction[];
  issue?: PackageErrorDto;
}
```

`allowed_actions` 由 Rust 根据平台、前置条件、安装来源、Runtime 状态和 operation 计算；前端不得自行推导权限。

## 13. 并发、取消与恢复

### 13.1 操作锁

- 全局最多一个 npm 安装事务；
- 本机扫描和网络版本查询可以并行；
- Local Runtime 正在 start/stop/restart 时不得 Pi Hub activation；
- 使用单调 generation id，旧事件不能覆盖新快照；
- 重复点击返回当前 operation，不启动第二个子进程。

### 13.2 取消

- 下载/安装阶段允许取消；
- manifest 原子提交开始后暂不可取消；
- 只终止 manager 持有的 npm 子进程组；
- 不按 PID、进程名或端口猜测进程；
- 取消后清理 staging，保留 active；
- App 退出请求取消并有界等待。

### 13.3 重启恢复

- staging 未提交：标记中断并清理；
- version 已就位、manifest 未切换：保留 inactive；
- manifest 已切换、Pi Hub 未验证：运行 detector/Doctor，失败提示回滚；
- 不自动重启或终止 external Pi Hub。

## 14. Tauri Commands 与事件

### 14.1 Commands

```rust
get_package_management_status()
scan_managed_products()
check_product_updates(force: bool)
start_product_install(product: ProductId, release_token: String)
start_product_update(product: ProductId, release_token: String)
confirm_pi_hub_update_restart(operation_id: Uuid)
cancel_package_operation(operation_id: Uuid)
activate_managed_product(product: ProductId, installation_id: Uuid)
get_package_operation_log(operation_id: Uuid, limit: Option<u32>)
```

- Command 只校验枚举/UUID/token 并调用 manager；
- 前端不能传 package、version spec、命令、参数、PID、路径或环境；
- token 绑定 product、exact version 和元数据，短期有效；
- iOS 返回稳定 `unsupported_platform`；
- 只注册给 Trusted main window。

### 14.2 Events

```text
package-management://status-changed
package-management://operation-changed
```

payload 只含 DTO，不含 registry/npm 原始输出、完整环境、凭据或未白名单路径。低频阶段事件用于 UI，不逐行广播 npm 输出。

## 15. 存储与迁移

建立独立 store：

```text
Application Support/Pi Hub Client/
├── profiles.json
├── local-runtime.json
└── package-management.json
```

`package-management.json` 含 `schema_version`、两个 active installation、Node 路径、package root 和最后检查时间，不含 Secret。

迁移规则：

1. Desktop 升级时不自动复制或移动外部安装；
2. 继续读取 V2 `local-runtime.json` 的 Pi Hub 路径；
3. 首次切换受管 Pi Hub 后，原子更新 package manifest 和 Runtime paths；
4. 两份 store 更新使用可恢复 journal；
5. 切回外部安装只改变选择，不删除受管版本；
6. Secret 不进入新 store。

## 16. 错误模型

| Error code | 含义 | 建议 |
|---|---|---|
| `package_platform_unsupported` | 平台不支持 | 不重试 |
| `package_operation_in_progress` | 已有互斥操作 | 稍后 |
| `package_node_unavailable` | Node 缺失/不兼容 | 修复前置条件 |
| `package_npm_unavailable` | npm 缺失/无效 | 修复前置条件 |
| `package_release_check_failed` | registry 查询失败 | 重试 |
| `package_release_invalid` | 元数据不合法 | 不重试 |
| `package_release_token_expired` | token 过期 | 重新检查 |
| `package_install_spawn_failed` | npm 无法启动 | 重试 |
| `package_install_failed` | npm 非零退出 | 查看摘要 |
| `package_install_timeout` | 安装超时 | 重试 |
| `package_verification_failed` | 后置验证失败 | 不激活 |
| `package_activation_failed` | 切换失败 | 回滚/重试 |
| `package_update_requires_restart` | 等待重启确认 | 用户确认 |
| `package_external_runtime_active` | 外部 Pi Hub 运行 | 等待外部停止 |
| `package_rollback_failed` | 恢复失败 | 人工处理 |
| `package_cancelled` | 用户取消 | 可重试 |
| `package_disk_space_insufficient` | 磁盘不足 | 清理后重试 |

错误 DTO 只含 product、stage、版本、required bytes 等白名单字段，不直接返回 npm stderr。

## 17. 安全与供应链

### 17.1 命令

- 禁止 `sh -c`、`zsh -c`、`bash -c`；
- 禁止通用 execute/spawn Tauri command；
- 只执行验证过的绝对 Node/npm CLI；
- 参数由 Rust 固定构造；
- stdin 关闭，输出有界并脱敏；
- 只终止 operation 持有的 child handle/进程组。

### 17.2 文件系统

- 写入必须位于 canonicalized 受管根；
- 拒绝 `..`、跨根 symlink 和非预期路径；
- package symlink 不得逃逸受管根；
- manifest/journal 原子写入；
- 不写入 `~/.pi/agent` 会话和认证数据；
- 更新不得清空或迁移用户 Session。

### 17.3 网络

- 只使用 HTTPS 和系统证书校验；
- 不允许忽略证书错误或 UI 任意下载 URL；
- registry/package 固定映射；
- npm audit 关闭，减少额外请求；
- 第一阶段只支持公开包，不读取/回传 npm token；
- 元数据和日志不得包含 Cookie/Authorization。

### 17.4 后置验证

**Pi：** package name、exact version、`bin.pi`、入口 canonical path、Node engine、`pi --version`。

**Pi Hub：** package name、exact version、`bin.pi-hub`、`.next` 生产构建、Node engine、`pi-hub --version --json` schema、离线 Doctor，以及启动后的 `/api/client-info`。

## 18. 日志与可观测性

每次操作保存有限结构化记录：

```text
operation_id, product, kind, from_version, target_version,
stage, started_at, finished_at, result_code, sanitized_summary
```

- 保留最近 10 次或 30 天；
- 单次日志限制行数和字节；
- 脱敏 Authorization、Cookie、Token、API Key、Private Key、npm token；
- 不记录完整环境、对话、Session、模型请求或凭据；
- 默认展示阶段和错误摘要，详细 npm 输出经脱敏后单独展示；
- 复制诊断再次经过白名单序列化。

## 19. 与现有模块集成

### 19.1 LocalRuntimeManager

增加最小适配接口（命名可调整）：

```rust
prepare_for_pi_hub_activation()
activate_verified_pi_hub(paths)
restart_after_package_activation()
restore_previous_pi_hub(paths)
```

必须保持：

- ProcessSupervisor 仍是进程所有权唯一真相；
- package manager 不自行 kill Pi Hub；
- 不复制端口探测和 ready 流程；
- LocalRuntimeManager 不联网查 npm；
- 激活后 Doctor cache 失效。

### 19.2 InstallationDetector

提取并复用 `probe_node`、package identity/bin 验证、candidate enumeration 和 engine/semver 判断，不重写两套扫描。V2 行为保持兼容，新模型通过 adapter 扩展。

### 19.3 Environment Doctor

更新后 force refresh。外部 Pi 仍不阻止 Pi Hub，但产品卡片独立显示管理状态；Doctor informational 项可链接到“设置 -> 本机组件 -> Pi”。

## 20. 代码改动清单

### 20.1 新文件

- `src-tauri/src/package_management/*`：包管理领域；
- `src-tauri/src/commands/package_management.rs`：薄 commands；
- `src/features/package-management/*`：设置 UI、DTO 和 API。

### 20.2 最小修改点

- `src-tauri/src/lib.rs`：注册 manager、commands、退出清理；
- `src-tauri/src/commands/mod.rs`：导出 command；
- `src-tauri/src/error.rs`：typed errors；
- `local_runtime/detector.rs`：复用验证、识别 managed roots；
- `local_runtime/manager.rs`：Pi Hub activation adapter；
- `local_runtime/settings.rs`：必要的 schema migration；
- `features/settings/SettingsPage.tsx`：增加设置导航；
- `app/App.tsx`：设置内部页面和确认弹窗；
- `LocalRuntimeSettingsPage.tsx`：显示 active managed path，避免 onBlur 覆盖事务结果。

remote Service WebView capability 保持为零；不得为了安装功能扩大其权限。

## 21. 测试设计

### 21.1 Rust 单元测试

- 两个包的 identity、bin、version、engine；
- managed/external 分类；
- not installed、invalid、incompatible 区分；
- semver、prerelease、newer-than-latest；
- registry 白名单、ETag、TTL、离线缓存；
- release token 绑定和过期；
- npm 与 Node 配对及固定参数无 Shell；
- staging/symlink 路径逃逸；
- manifest 原子写、迁移、损坏恢复；
- generation、重复操作、取消；
- 日志上限和 Secret 脱敏；
- 旧版本引用保护和清理。

### 21.2 Rust 集成测试

1. 两者均缺失，一键安装成功；
2. Pi 成功、Pi Hub 失败，结果部分完成；
3. 初装 npm 失败，active manifest 不存在；
4. 更新下载失败，旧版本保持 active；
5. 包名/bin 不符，拒绝激活；
6. Pi Hub `.next` 缺失，拒绝激活；
7. running_managed 更新重启成功；
8. 新版启动失败并回滚；
9. running_external 拒绝 stop/activate；
10. port conflict 不宣称完成；
11. 取消只终止受管 npm child；
12. App 中断后 journal 恢复；
13. registry 超时仍展示本机状态；
14. Node 可用、npm 缺失时正确阻断。

### 21.3 前端测试

- 设置导航和 macOS-only 入口；
- 两个卡片的全部状态；
- 两者均缺失时一键安装；
- Node/npm 阻断；
- 更新确认与“更新并重启”；
- external Pi Hub 不显示强制切换；
- 防重复点击、取消和旧 generation 过滤；
- 离线不清空安装信息；
- 部分完成、回滚成功/失败；
- iOS 不显示入口。

### 21.4 macOS 真机

- Apple Silicon Homebrew Node；
- Intel `/usr/local` Node；
- NVM、Volta、FNM；
- Finder 启动签名 App；
- 两者均无、只有一个、均有外部旧版本、已有受管版本；
- Node 过低、npm 缺失；
- 无网络、慢网络、registry 5xx；
- 磁盘不足和目录不可写；
- Pi Hub managed/external/port conflict；
- 成功、取消、失败和回滚；
- Developer ID 签名、公证、DMG 后的目录权限。

### 21.5 iOS 回归

iOS build、Direct URL、SSH Forward、Host Key、Viewer、Keychain；不显示本机组件，commands 返回 stable unsupported error。

## 22. 开发阶段

### Phase 0：范围与契约

- [x] 登记 V3（`README.md`、`AGENTS.md` §1）；
- [x] 修订 `AGENTS.md` 中安装/升级的版本范围（`AGENTS.md` §2.3、§9；新增 §5.6、§6.6 V3 子句）；
- [x] 更新 requirements（新增 `docs/requirements-v3.md`；修订 `requirements-v2.md` §5.2）；
- [ ] 确认两个发布包支持 `--ignore-scripts`（依赖 `jiangliuhong/pi-hub` 与 `@earendil-works/pi-coding-agent` 仓库）；
- [ ] 确认版本命令稳定 schema（同上）。

> 治理部分已完成；两项跨仓库确认是 Phase 1 功能代码的硬门控，未关闭前不得提交依赖它们的安装/后置验证代码。

### Phase 1：只读状态

- 强化 Pi 检测；
- 每产品状态模型；
- release 查询和缓存；
- 设置页展示安装/版本/更新状态；
- 不执行安装写入。

### Phase 2：初次安装

- npm toolchain detector；
- managed store、staging、journal；
- 独立安装和一键安装；
- 取消、日志和后置验证。

### Phase 3：更新与回滚

- Pi 更新；
- stopped Pi Hub 更新；
- running_managed 更新并重启；
- external 等待切换；
- 启动失败自动回滚；
- 旧版本清理。

### Phase 4：分发验收

- 真机矩阵；
- 签名、公证、DMG；
- 网络/磁盘异常；
- iOS 回归；
- README、用户帮助和诊断文档。

## 23. 验收标准

1. 设置页分别展示 Pi/Pi Hub 安装状态、版本、来源和路径。
2. 联网可检查 stable 最新版本；离线时本机状态仍准确。
3. 任一产品缺失时可完成 Desktop 受管初次安装。
4. 两者均缺失且 Node/npm 满足时，可分别或一键安装。
5. 任一产品有更新时可更新到精确 stable 版本。
6. 外部安装更新不修改 Homebrew/NVM/Volta，而是安装受管副本。
7. 不使用 Shell、不请求 sudo、不接受任意 package/path/args。
8. 失败、验证失败或取消不破坏 active 版本。
9. running_managed Pi Hub 更新需确认并复用现有 supervisor。
10. running_external Pi Hub 不会被停止或 Kill。
11. 新 Pi Hub 启动失败时恢复旧 active 版本并报告结果。
12. 安装有阶段、取消、有限脱敏日志和稳定错误码。
13. Service WebView 无法调用包管理命令。
14. iOS 编译和 V1 不受影响且不显示入口。

## 24. 实现前必须确认

1. 两个 npm package 的所有发布物是否都能在 `--ignore-scripts` 下完整运行。
2. 是否接受受管 Pi 默认不写入终端 PATH；PATH 集成需要单独授权设计。
3. registry 是否固定为官方公共 registry；镜像/代理需独立信任和凭据设计。
4. Pi Hub 更新后的数据格式是否向前兼容；若有不可逆迁移，回滚需增加数据兼容检查。
5. 第一阶段固定 stable only；beta channel 留待后续。

## 25. Definition of Done

- V3 requirements、本文档和 `AGENTS.md` 范围一致；
- 检测、版本查询、安装、更新、取消、回滚均有 typed state 和测试；
- 外部安装和 external Pi Hub 未被越权修改或终止；
- active 切换原子，异常退出可恢复；
- 不泄露 registry/npm/环境/认证 Secret；
- Pi Hub 更新复用 Local Runtime 和 service identity 契约；
- macOS 真机矩阵完成并记录；
- iOS build 与 V1 回归通过；
- 所有执行和未执行的检查如实记录。
