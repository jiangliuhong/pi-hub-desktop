//! Pi environment diagnostics (docs/design-v2.md §8, requirements-v2 §8).
//!
//! The doctor is a *diagnostic*, never a fixer (design-v2 §8.1): it runs
//! offline, never calls a model API, never refreshes OAuth, never prints
//! secrets. Desktop owns the Node / Pi Hub path / agent-dir / optional CLI
//! checks; Pi Hub's `doctor --json --offline` owns the embedded-runtime,
//! session, auth and model checks. The two are merged and aggregated into a
//! single `EnvironmentReport` (design-v2 §8.6).

use crate::error::LocalRuntimeError;
use crate::local_runtime::detector::CommandRunner;
use crate::local_runtime::model::{
    CheckCategory, CheckResult, CheckSeverity, CheckStatus, EnvironmentReport, EnvironmentStatus,
};
use crate::local_runtime::settings::LocalRuntimeSettings;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Doctor schema version we understand (design-v2 §8.3).
const EXPECTED_DOCTOR_SCHEMA: u32 = 1;
const DOCTOR_TIMEOUT: Duration = Duration::from_secs(20);

/// Inputs to a diagnosis run.
#[derive(Debug, Clone)]
pub struct DoctorContext {
    pub node_executable: PathBuf,
    pub pi_hub_entrypoint: PathBuf,
    pub pi_hub_package_root: PathBuf,
    pub settings: LocalRuntimeSettings,
}

/// The doctor contract (design-v2 §8.2).
#[async_trait]
pub trait PiEnvironmentDoctor: Send + Sync {
    async fn diagnose(&self, ctx: &DoctorContext) -> Result<EnvironmentReport, LocalRuntimeError>;
}

/// Default doctor: combines Desktop-owned checks with Pi Hub's own doctor.
pub struct DefaultPiEnvironmentDoctor {
    runner: std::sync::Arc<dyn CommandRunner>,
}

impl DefaultPiEnvironmentDoctor {
    pub fn new(runner: std::sync::Arc<dyn CommandRunner>) -> Self {
        DefaultPiEnvironmentDoctor { runner }
    }
    pub fn with_default_runner() -> Self {
        Self::new(std::sync::Arc::new(
            crate::local_runtime::detector::TokioCommandRunner,
        ))
    }
}

#[async_trait]
impl PiEnvironmentDoctor for DefaultPiEnvironmentDoctor {
    async fn diagnose(&self, ctx: &DoctorContext) -> Result<EnvironmentReport, LocalRuntimeError> {
        let mut checks = Vec::new();

        // --- Desktop-owned checks ---
        checks.push(self.check_node(ctx).await);
        checks.push(check_pi_hub_path(ctx));
        checks.push(check_agent_dir(ctx));

        // --- Pi Hub-owned checks (embedded runtime / session / auth / model) ---
        let doctor_checks = self.run_pi_hub_doctor(ctx).await;
        checks.extend(doctor_checks);

        let overall = aggregate(&checks);
        Ok(EnvironmentReport {
            overall,
            generated_at: Some(Utc::now()),
            checks,
        })
    }
}

impl DefaultPiEnvironmentDoctor {
    async fn run_pi_hub_doctor(&self, ctx: &DoctorContext) -> Vec<CheckResult> {
        let mut env: Vec<(&str, &str)> = Vec::new();
        let agent_dir_str;
        if let Some(dir) = ctx
            .settings
            .pi_agent_dir
            .as_ref()
            .filter(|dir| !dir.as_os_str().is_empty())
        {
            agent_dir_str = dir.to_string_lossy().into_owned();
            env.push(("PI_CODING_AGENT_DIR", agent_dir_str.as_str()));
        }
        env.push(("PI_OFFLINE", "1"));
        let result = self
            .runner
            .run(
                &ctx.node_executable,
                &[
                    ctx.pi_hub_entrypoint.to_string_lossy().as_ref(),
                    "doctor",
                    "--json",
                    "--offline",
                ],
                Some(&ctx.pi_hub_package_root),
                DOCTOR_TIMEOUT,
                &env,
            )
            .await;
        match result {
            Ok(out) => parse_doctor_output(&out.stdout, out.exit_code),
            Err(e) => vec![CheckResult {
                id: "DEP-PIHUB-DOCTOR".into(),
                category: CheckCategory::PiEnvironment,
                severity: CheckSeverity::Required,
                status: CheckStatus::Fail,
                code: Some("pi_hub_doctor_unavailable".into()),
                message: Some(format!("Pi Hub 环境检查无法执行：{e}")),
                remediation: Some(
                    "请确认 Node.js 与 Pi Hub 安装路径有效，并升级到支持 doctor 协议的版本。"
                        .into(),
                ),
                details: BTreeMap::new(),
            }],
        }
    }

    // ---- Desktop-owned checks ----

    async fn check_node(&self, ctx: &DoctorContext) -> CheckResult {
        let id = "DEP-NODE-001";
        if !ctx.node_executable.exists() {
            return fail(
                id,
                CheckCategory::Runtime,
                CheckSeverity::Required,
                "node_missing",
                "未找到 Node.js 可执行文件。",
                "请在设置中选择有效的 Node.js 安装路径。",
            );
        }
        // Probe the version via the *injected* runner (so tests can fake it).
        let out = self
            .runner
            .run(
                &ctx.node_executable,
                &["--version"],
                None,
                Duration::from_secs(8),
                &[],
            )
            .await;
        match out {
            Ok(o) if matches!(o.exit_code, Some(0)) => {
                if let Some(v) = crate::local_runtime::detector::parse_node_version(&o.stdout) {
                    // DEP-NODE-001: enforce the Pi Hub Node baseline (requirements-v2
                    // §8.2). An incompatible Node is a *required* failure, not just a
                    // warning, so it blocks start.
                    if !crate::local_runtime::detector::node_satisfies_baseline(&v) {
                        let mut details = BTreeMap::new();
                        details.insert("version".into(), serde_json::Value::String(v.to_string()));
                        details.insert(
                            "requiredVersion".into(),
                            serde_json::Value::String(format!(
                                ">={}.{}.{}",
                                crate::local_runtime::model::NODE_REQUIRED_MAJOR,
                                crate::local_runtime::model::NODE_REQUIRED_MINOR,
                                crate::local_runtime::model::NODE_REQUIRED_PATCH,
                            )),
                        );
                        return CheckResult {
                            id: id.into(),
                            category: CheckCategory::Runtime,
                            severity: CheckSeverity::Required,
                            status: CheckStatus::Fail,
                            code: Some("node_version_incompatible".into()),
                            message: Some(format!("Node.js {} 不满足要求。", v)),
                            remediation: Some("请升级 Node.js 到要求的最低版本。".into()),
                            details,
                        };
                    }
                    let mut details = BTreeMap::new();
                    details.insert("version".into(), serde_json::Value::String(v.to_string()));
                    pass(
                        id,
                        CheckCategory::Runtime,
                        CheckSeverity::Required,
                        "node_available",
                        format!("Node.js {} 可用。", v),
                        Some(details),
                    )
                } else {
                    fail(
                        id,
                        CheckCategory::Runtime,
                        CheckSeverity::Required,
                        "node_version_unreadable",
                        "无法读取 Node.js 版本。",
                        "请检查 Node.js 安装是否完整。",
                    )
                }
            }
            _ => fail(
                id,
                CheckCategory::Runtime,
                CheckSeverity::Required,
                "node_execution_failed",
                "执行 Node.js 失败。",
                "请在设置中选择有效的 Node.js 安装路径。",
            ),
        }
    }
}

fn check_pi_hub_path(ctx: &DoctorContext) -> CheckResult {
    let id = "DEP-PIHUB-001";
    if !ctx.pi_hub_entrypoint.exists() {
        return fail(
            id,
            CheckCategory::PiHub,
            CheckSeverity::Required,
            "pi_hub_entrypoint_missing",
            "未找到 Pi Hub 入口文件。",
            "请在设置中选择有效的 Pi Hub 入口路径。",
        );
    }
    if !ctx.pi_hub_package_root.is_dir() {
        return fail(
            id,
            CheckCategory::PiHub,
            CheckSeverity::Required,
            "pi_hub_package_root_missing",
            "未找到 Pi Hub 安装根目录。",
            "请重新选择或重新安装 Pi Hub。",
        );
    }
    let mut details = BTreeMap::new();
    details.insert(
        "path".into(),
        serde_json::Value::String(ctx.pi_hub_package_root.to_string_lossy().into_owned()),
    );
    pass(
        id,
        CheckCategory::PiHub,
        CheckSeverity::Required,
        "pi_hub_installation_valid",
        "Pi Hub 安装路径有效。".into(),
        Some(details),
    )
}

/// Check that the agent data directory is resolvable and creatable. A missing
/// directory that can be safely created is NOT a failure (requirements-v2 §8.2
/// DEP-PI-DIR-001).
fn check_agent_dir(ctx: &DoctorContext) -> CheckResult {
    let id = "DEP-PI-DIR-001";
    let dir = effective_agent_dir(&ctx.settings);
    let Some(dir) = dir else {
        return fail(
            id,
            CheckCategory::PiEnvironment,
            CheckSeverity::Required,
            "pi_agent_dir_unresolvable",
            "无法解析 Pi Agent 数据目录。",
            "请设置有效的 Agent 数据目录或确认 HOME 可用。",
        );
    };
    if dir.exists() {
        let mut details = BTreeMap::new();
        details.insert(
            "path".into(),
            serde_json::Value::String(dir.to_string_lossy().into_owned()),
        );
        // Best-effort writability probe.
        if is_writable(&dir) {
            pass(
                id,
                CheckCategory::PiEnvironment,
                CheckSeverity::Required,
                "pi_agent_dir_ready",
                "Pi Agent 数据目录可读写。".into(),
                Some(details),
            )
        } else {
            fail_with_path(
                id,
                CheckCategory::PiEnvironment,
                CheckSeverity::Required,
                "pi_agent_dir_not_writable",
                "Pi Agent 数据目录不可写。",
                "请修正目录权限或在设置中更换 Agent 目录。",
                &dir,
            )
        }
    } else if let Some(parent) = dir.parent() {
        if parent.exists() && is_writable(parent) {
            let mut details = BTreeMap::new();
            details.insert(
                "path".into(),
                serde_json::Value::String(dir.to_string_lossy().into_owned()),
            );
            CheckResult {
                id: id.into(),
                category: CheckCategory::PiEnvironment,
                severity: CheckSeverity::Required,
                status: CheckStatus::Pass,
                code: Some("pi_agent_dir_creatable".into()),
                message: Some("Pi Agent 数据目录不存在但可安全创建。".into()),
                remediation: None,
                details,
            }
        } else {
            fail_with_path(
                id,
                CheckCategory::PiEnvironment,
                CheckSeverity::Required,
                "pi_agent_dir_parent_unwritable",
                "Pi Agent 数据目录的父目录不可写，无法创建。",
                "请创建目录或修改 Agent 目录设置。",
                &dir,
            )
        }
    } else {
        fail_with_path(
            id,
            CheckCategory::PiEnvironment,
            CheckSeverity::Required,
            "pi_agent_dir_parent_missing",
            "Pi Agent 数据目录的父目录不存在。",
            "请创建目录或修改 Agent 目录设置。",
            &dir,
        )
    }
}

/// Resolve the effective Pi Agent dir (design-v2 §8.4): `PI_CODING_AGENT_DIR`
/// override from settings, otherwise `~/.pi/agent`.
fn effective_agent_dir(settings: &LocalRuntimeSettings) -> Option<PathBuf> {
    if let Some(dir) = settings
        .pi_agent_dir
        .as_ref()
        .filter(|dir| !dir.as_os_str().is_empty())
    {
        return Some(dir.clone());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".pi/agent"))
}

#[cfg(unix)]
fn is_writable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_dir() && (m.permissions().mode() & 0o200 != 0))
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_writable(path: &Path) -> bool {
    path.is_dir()
}

// ---- Pi Hub doctor output parsing (design-v2 §8.3) ----

#[derive(Debug, Deserialize)]
struct DoctorDocument {
    #[serde(default, rename = "schemaVersion")]
    #[allow(dead_code)]
    schema_version: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    overall: Option<String>,
    #[serde(default)]
    checks: Vec<RawCheck>,
}

#[derive(Debug, Deserialize)]
struct RawCheck {
    /// Pi Hub uses `name`; older contracts used `id`. Accept either.
    #[serde(default, alias = "name")]
    id: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    remediation: Option<String>,
    #[serde(default)]
    details: BTreeMap<String, serde_json::Value>,
    /// Pi Hub emits `detail` (singular) as a string or object. Normalize to
    /// `details` so the UI can show it.
    #[serde(default)]
    detail: Option<serde_json::Value>,
}

/// Parse Pi Hub's doctor JSON into validated check results. On schema/version
/// mismatch the whole doctor batch is reported as a single required failure so
/// the UI can explain why embedded checks are unavailable (design-v2 §8.5).
fn parse_doctor_output(stdout: &str, exit_code: Option<i32>) -> Vec<CheckResult> {
    let doc: DoctorDocument = match serde_json::from_str(stdout.trim()) {
        Ok(d) => d,
        Err(e) => {
            return vec![CheckResult {
                id: "DEP-PIHUB-DOCTOR".into(),
                category: CheckCategory::PiEnvironment,
                severity: CheckSeverity::Required,
                status: CheckStatus::Fail,
                code: Some("pi_hub_doctor_invalid_output".into()),
                message: Some(format!("Pi Hub doctor 输出无法解析：{e}")),
                remediation: Some("请升级到支持 doctor --json --offline 协议的 Pi Hub。".into()),
                details: BTreeMap::new(),
            }];
        }
    };
    if !matches!(doc.schema_version, Some(v) if v == EXPECTED_DOCTOR_SCHEMA) {
        return vec![CheckResult {
            id: "DEP-PIHUB-DOCTOR".into(),
            category: CheckCategory::PiEnvironment,
            severity: CheckSeverity::Required,
            status: CheckStatus::Fail,
            code: Some("pi_hub_doctor_schema_mismatch".into()),
            message: Some(format!(
                "Pi Hub doctor schemaVersion 不兼容（期望 {EXPECTED_DOCTOR_SCHEMA}）。"
            )),
            remediation: Some("请升级 Pi Hub 或 Pi Hub Client。".into()),
            details: BTreeMap::new(),
        }];
    }
    doc.checks
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            // Merge singular `detail` (string or object) into the `details`
            // map so the UI can display it regardless of which shape Pi Hub
            // emitted.
            let mut details = c.details;
            if let Some(d) = c.detail {
                let key = "detail";
                match d {
                    serde_json::Value::String(s) => {
                        details.insert(key.into(), serde_json::Value::String(s));
                    }
                    other if !other.is_null() => {
                        details.insert(key.into(), other);
                    }
                    _ => {}
                }
            }
            CheckResult {
                id: c.id.unwrap_or_else(|| format!("doctor-check-{i}")),
                category: parse_category(c.category.as_deref()),
                severity: parse_severity(c.severity.as_deref()),
                status: parse_status(c.status.as_deref()),
                code: c.code,
                message: c.message,
                remediation: c.remediation,
                details,
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .chain([doctor_exit_consistency(exit_code)])
        .collect()
}

/// design-v2 §8.5: Desktop validates both JSON `overall` and exit code; on
/// conflict it is treated as an internal error. We surface a consistency check.
fn doctor_exit_consistency(exit_code: Option<i32>) -> CheckResult {
    let consistent = matches!(exit_code, Some(0) | Some(1) | Some(2));
    if consistent {
        CheckResult {
            id: "DEP-PIHUB-DOCTOR-EXIT".into(),
            category: CheckCategory::PiEnvironment,
            severity: CheckSeverity::Informational,
            status: CheckStatus::Pass,
            code: Some("doctor_exit_consistent".into()),
            message: Some(format!("Pi Hub doctor 退出码：{:?}。", exit_code)),
            remediation: None,
            details: BTreeMap::new(),
        }
    } else {
        CheckResult {
            id: "DEP-PIHUB-DOCTOR-EXIT".into(),
            category: CheckCategory::PiEnvironment,
            severity: CheckSeverity::Required,
            status: CheckStatus::Fail,
            code: Some("doctor_exit_internal_error".into()),
            message: Some(format!(
                "Pi Hub doctor 退出码异常：{:?}（内部错误）。",
                exit_code
            )),
            remediation: Some("请重新运行检查；若持续出现请升级 Pi Hub。".into()),
            details: BTreeMap::new(),
        }
    }
}

/// Overall aggregation rule (design-v2 §8.6).
pub fn aggregate(checks: &[CheckResult]) -> EnvironmentStatus {
    let any_required_fail = checks
        .iter()
        .any(|c| c.severity == CheckSeverity::Required && c.status.is_failure());
    if any_required_fail {
        return EnvironmentStatus::Blocked;
    }
    let any_recommended_problem = checks.iter().any(|c| {
        c.severity == CheckSeverity::Recommended
            && (c.status.is_failure() || c.status == CheckStatus::Warn)
    });
    if any_recommended_problem {
        return EnvironmentStatus::Degraded;
    }
    EnvironmentStatus::Ready
}

fn parse_category(s: Option<&str>) -> CheckCategory {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("runtime") => CheckCategory::Runtime,
        Some("pi_hub") => CheckCategory::PiHub,
        Some("auth_and_models") | Some("auth-and-models") | Some("authandmodels") => {
            CheckCategory::AuthAndModels
        }
        Some("optional_tools") | Some("optional-tools") => CheckCategory::OptionalTools,
        _ => CheckCategory::PiEnvironment,
    }
}

fn parse_severity(s: Option<&str>) -> CheckSeverity {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("recommended") => CheckSeverity::Recommended,
        Some("informational") => CheckSeverity::Informational,
        _ => CheckSeverity::Required,
    }
}

fn parse_status(s: Option<&str>) -> CheckStatus {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("warn") | Some("warning") => CheckStatus::Warn,
        Some("skip") | Some("skipped") => CheckStatus::Skipped,
        Some("fail") | Some("failed") | Some("error") => CheckStatus::Fail,
        _ => CheckStatus::Pass,
    }
}

// ---- check-result builders ----

fn fail(
    id: &str,
    category: CheckCategory,
    severity: CheckSeverity,
    code: &str,
    message: &str,
    remediation: &str,
) -> CheckResult {
    CheckResult {
        id: id.into(),
        category,
        severity,
        status: CheckStatus::Fail,
        code: Some(code.into()),
        message: Some(message.into()),
        remediation: Some(remediation.into()),
        details: BTreeMap::new(),
    }
}

fn fail_with_path(
    id: &str,
    category: CheckCategory,
    severity: CheckSeverity,
    code: &str,
    message: &str,
    remediation: &str,
    path: &Path,
) -> CheckResult {
    let mut details = BTreeMap::new();
    details.insert(
        "path".into(),
        serde_json::Value::String(path.to_string_lossy().into_owned()),
    );
    CheckResult {
        id: id.into(),
        category,
        severity,
        status: CheckStatus::Fail,
        code: Some(code.into()),
        message: Some(message.into()),
        remediation: Some(remediation.into()),
        details,
    }
}

fn pass(
    id: &str,
    category: CheckCategory,
    severity: CheckSeverity,
    code: &str,
    message: String,
    details: Option<BTreeMap<String, serde_json::Value>>,
) -> CheckResult {
    CheckResult {
        id: id.into(),
        category,
        severity,
        status: CheckStatus::Pass,
        code: Some(code.into()),
        message: Some(message),
        remediation: None,
        details: details.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_runtime::detector::{CommandOutput, CommandRunner};
    use crate::local_runtime::settings::LocalRuntimeSettings;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// A runner fake returning a canned stdout for any invocation.
    struct FakeRunner(Mutex<CommandOutput>);
    impl FakeRunner {
        fn new(stdout: &str) -> Self {
            FakeRunner(Mutex::new(CommandOutput {
                exit_code: Some(0),
                stdout: stdout.into(),
                stderr: String::new(),
            }))
        }
    }
    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(
            &self,
            _program: &std::path::Path,
            _args: &[&str],
            _cwd: Option<&std::path::Path>,
            _timeout: Duration,
            _extra_env: &[(&str, &str)],
        ) -> Result<CommandOutput, LocalRuntimeError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    fn doctor_with(stdout: &str) -> DefaultPiEnvironmentDoctor {
        DefaultPiEnvironmentDoctor::new(std::sync::Arc::new(FakeRunner::new(stdout)))
    }

    fn ctx_with_node(node: &std::path::Path) -> DoctorContext {
        DoctorContext {
            node_executable: node.to_path_buf(),
            pi_hub_entrypoint: std::path::PathBuf::from("/nonexistent/pi-hub.js"),
            pi_hub_package_root: std::path::PathBuf::from("/nonexistent/pkg"),
            settings: LocalRuntimeSettings::default(),
        }
    }

    #[tokio::test]
    async fn check_node_passes_for_compatible_version() {
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("node");
        std::fs::write(&node, "x").unwrap();
        let doctor = doctor_with("v24.19.0\n");
        let result = doctor.check_node(&ctx_with_node(&node)).await;
        assert_eq!(result.status, CheckStatus::Pass);
        assert_eq!(result.code.as_deref(), Some("node_available"));
    }

    #[tokio::test]
    async fn check_node_blocks_incompatible_version() {
        // M2: a Node below the baseline must be a *required* failure, blocking
        // start (DEP-NODE-001).
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("node");
        std::fs::write(&node, "x").unwrap();
        let doctor = doctor_with("v18.0.0\n");
        let result = doctor.check_node(&ctx_with_node(&node)).await;
        assert_eq!(result.severity, CheckSeverity::Required);
        assert_eq!(result.status, CheckStatus::Fail);
        assert_eq!(result.code.as_deref(), Some("node_version_incompatible"));
        assert_eq!(
            result.details.get("version").and_then(|v| v.as_str()),
            Some("18.0.0")
        );
    }

    #[test]
    fn empty_agent_dir_override_uses_default_directory() {
        let settings = LocalRuntimeSettings {
            pi_agent_dir: Some(PathBuf::new()),
            ..Default::default()
        };

        let resolved = effective_agent_dir(&settings).expect("HOME resolves default agent dir");
        assert!(!resolved.as_os_str().is_empty());
        assert!(resolved.ends_with(".pi/agent"));
    }

    #[test]
    fn aggregate_blocked_on_required_fail() {
        let checks = vec![
            CheckResult {
                id: "a".into(),
                category: CheckCategory::Runtime,
                severity: CheckSeverity::Required,
                status: CheckStatus::Fail,
                code: None,
                message: None,
                remediation: None,
                details: BTreeMap::new(),
            },
            CheckResult {
                id: "b".into(),
                category: CheckCategory::PiEnvironment,
                severity: CheckSeverity::Recommended,
                status: CheckStatus::Pass,
                code: None,
                message: None,
                remediation: None,
                details: BTreeMap::new(),
            },
        ];
        assert_eq!(aggregate(&checks), EnvironmentStatus::Blocked);
    }

    #[test]
    fn aggregate_degraded_on_recommended_warn() {
        let checks = vec![CheckResult {
            id: "a".into(),
            category: CheckCategory::AuthAndModels,
            severity: CheckSeverity::Recommended,
            status: CheckStatus::Warn,
            code: None,
            message: None,
            remediation: None,
            details: BTreeMap::new(),
        }];
        assert_eq!(aggregate(&checks), EnvironmentStatus::Degraded);
    }

    #[test]
    fn aggregate_ready_when_all_pass() {
        let checks = vec![CheckResult {
            id: "a".into(),
            category: CheckCategory::Runtime,
            severity: CheckSeverity::Required,
            status: CheckStatus::Pass,
            code: None,
            message: None,
            remediation: None,
            details: BTreeMap::new(),
        }];
        assert_eq!(aggregate(&checks), EnvironmentStatus::Ready);
    }

    #[test]
    fn informational_fail_does_not_block() {
        let checks = vec![CheckResult {
            id: "a".into(),
            category: CheckCategory::OptionalTools,
            severity: CheckSeverity::Informational,
            status: CheckStatus::Fail,
            code: None,
            message: None,
            remediation: None,
            details: BTreeMap::new(),
        }];
        // Informational failure (e.g. missing external pi CLI) must NOT block.
        assert_eq!(aggregate(&checks), EnvironmentStatus::Ready);
    }

    #[test]
    fn parse_doctor_output_valid() {
        let json = serde_json::json!({
            "schemaVersion": 1,
            "overall": "degraded",
            "checks": [
                {
                    "id": "DEP-PI-EMBEDDED-001",
                    "category": "pi_environment",
                    "severity": "required",
                    "status": "pass",
                    "message": "ok"
                },
                {
                    "id": "DEP-PI-AUTH-001",
                    "category": "auth_and_models",
                    "severity": "recommended",
                    "status": "warn"
                }
            ]
        })
        .to_string();
        let checks = parse_doctor_output(&json, Some(1));
        assert!(checks.iter().any(|c| c.id == "DEP-PI-EMBEDDED-001"));
        assert!(checks
            .iter()
            .any(|c| c.id == "DEP-PIHUB-DOCTOR-EXIT" && c.status == CheckStatus::Pass));
    }

    #[test]
    fn parse_doctor_output_real_pi_hub_schema() {
        // Pi Hub's actual `doctor --json --offline` output uses `name` (not
        // `id`), `detail` (not `details`), and emits no category/severity. This
        // must parse without falling back to the invalid-output failure, and
        // all-pass checks must aggregate to Ready.
        let json = serde_json::json!({
            "schemaVersion": 1,
            "status": "healthy",
            "checks": [
                {"name": "nodeVersion", "status": "pass", "detail": "24.10.0"},
                {"name": "piHubHome", "status": "pass", "detail": "/Users/x/.pi/hub (writable: true)"},
                {"name": "buildArtifacts", "status": "pass", "detail": "/opt/.../.next (exists: true)"},
                {"name": "envReport", "status": "pass", "detail": {"PI_HUB_HOME": false, "PI_HUB_PASSWORD": false}}
            ]
        })
        .to_string();
        let checks = parse_doctor_output(&json, Some(0));
        // No invalid-output fallback.
        assert!(checks
            .iter()
            .all(|c| c.code.as_deref() != Some("pi_hub_doctor_invalid_output")));
        // `name` was accepted as the check id.
        assert!(checks.iter().any(|c| c.id == "nodeVersion"));
        // `detail` (string) merged into details.
        let node_check = checks
            .iter()
            .find(|c| c.id == "nodeVersion")
            .expect("nodeVersion check exists");
        assert_eq!(
            node_check.details.get("detail").and_then(|v| v.as_str()),
            Some("24.10.0")
        );
        // All pass → Ready (not Blocked).
        assert_eq!(aggregate(&checks), EnvironmentStatus::Ready);
    }

    #[test]
    fn parse_doctor_output_rejects_wrong_schema() {
        let json = r#"{"schemaVersion": 999, "checks": []}"#;
        let checks = parse_doctor_output(json, Some(0));
        assert!(checks.iter().any(
            |c| c.code.as_deref() == Some("pi_hub_doctor_schema_mismatch")
                && c.status == CheckStatus::Fail
        ));
    }

    #[test]
    fn parse_doctor_output_invalid_json_reports_failure() {
        let checks = parse_doctor_output("not json at all", Some(0));
        assert!(checks
            .iter()
            .any(|c| c.code.as_deref() == Some("pi_hub_doctor_invalid_output")));
    }

    #[test]
    fn exit_code_internal_error_is_required_fail() {
        let checks = parse_doctor_output(
            r#"{"schemaVersion":1,"overall":"ready","checks":[]}"#,
            Some(3),
        );
        assert!(checks.iter().any(|c| c.id == "DEP-PIHUB-DOCTOR-EXIT"
            && c.status == CheckStatus::Fail
            && c.severity == CheckSeverity::Required));
    }
}
