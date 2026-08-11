import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import type { ProfileDraft } from "./api";
import { createService, getService, updateService } from "./api";
import type { ServiceProfile, SshAuthType, ServiceScheme } from "./model";
import { putCredential } from "../credentials/api";
import type { CredentialKind } from "../credentials/api";
import { generateCredentialId } from "../credentials/model";
import {
  isPlaintextDirectUrl,
  validateDirectUrl,
  validateHost,
  validateName,
  validatePort,
  validatePrivateKeyPem,
  validateSshUsername,
  type ValidationResult,
} from "../../lib/validation";
import type { AppErrorDto } from "../../lib/tauri";

/**
 * Service create/edit form (docs/requirements-v1.md §8.2, FR-002/FR-003).
 *
 * Fields render conditionally on `connection_type` (never one bag of nullable
 * fields). SSH secrets are written to Keychain and only the credential id is
 * stored on the profile (AGENTS.md §6.1). Input state holding secrets is
 * cleared immediately after the Keychain write (§10.2).
 */
export interface ServiceFormPageProps {
  mode: "create" | "edit";
}

interface FormState {
  name: string;
  connectionType: "direct_url" | "ssh_forward";
  baseUrl: string;
  sshHost: string;
  sshPort: number;
  sshUsername: string;
  sshAuthType: SshAuthType;
  sshPassword: string;
  privateKey: string;
  privateKeyPassphrase: string;
  piHubPassword: string;
  targetHost: string;
  targetPort: number;
  serviceScheme: ServiceScheme;
  serviceBasePath: string;
}

const DEFAULTS: FormState = {
  name: "",
  connectionType: "direct_url",
  baseUrl: "",
  sshHost: "",
  sshPort: 22,
  sshUsername: "",
  sshAuthType: "password",
  sshPassword: "",
  privateKey: "",
  privateKeyPassphrase: "",
  piHubPassword: "",
  targetHost: "127.0.0.1",
  targetPort: 30142,
  serviceScheme: "http",
  serviceBasePath: "/",
};

export function ServiceFormPage({ mode }: ServiceFormPageProps) {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [form, setForm] = useState<FormState>(DEFAULTS);
  const [loadedExisting, setLoadedExisting] = useState(mode === "create");
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [submitting, setSubmitting] = useState(false);
  const [fatal, setFatal] = useState<string | null>(null);

  // Load existing profile once for edit mode (no secrets — only references).
  if (mode === "edit" && !loadedExisting) {
    // Defer the async load via an effect-like guard to keep the component pure
    // in tests; the body uses a synchronous flag so it runs once.
    if (id) {
      void getService(id)
        .then((p) => {
          setForm(profileToForm(p));
        })
        .catch((e) => setFatal(toMessage(e)))
        .finally(() => setLoadedExisting(true));
    } else {
      setLoadedExisting(true);
    }
  }

  const update = <K extends keyof FormState>(key: K, value: FormState[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const validate = (): Record<string, string> => {
    const next: Record<string, string> = {};
    const set = (key: string, r: ValidationResult) => {
      if (!r.ok) next[key] = r.reason;
    };
    set("name", validateName(form.name));
    if (form.connectionType === "direct_url") {
      set("baseUrl", validateDirectUrl(form.baseUrl));
    } else {
      set("sshHost", validateHost(form.sshHost));
      set("sshPort", validatePort(form.sshPort));
      set("sshUsername", validateSshUsername(form.sshUsername));
      set("targetPort", validatePort(form.targetPort));
      if (form.sshAuthType === "private_key") {
        set("privateKey", validatePrivateKeyPem(form.privateKey));
      } else {
        if (!form.sshPassword) next.sshPassword = "密码不能为空";
      }
    }
    return next;
  };

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    const found = validate();
    setErrors(found);
    if (Object.keys(found).length > 0) return;

    setSubmitting(true);
    setFatal(null);
    try {
      const draft = await buildDraft(form);
      const saved =
        mode === "create"
          ? await createService(draft)
          : await updateService(id ?? "", draft);
      // Clear secret inputs immediately after the Keychain write (AGENTS.md §10.2).
      setForm((prev) => ({
        ...prev,
        sshPassword: "",
        privateKey: "",
        privateKeyPassphrase: "",
        piHubPassword: "",
      }));
      void navigate(`/connect/${saved.metadata.id}`);
    } catch (e) {
      setFatal(toMessage(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <form className="service-form" onSubmit={(e) => void handleSubmit(e)}>
      <h2>{mode === "create" ? "添加服务" : "编辑服务"}</h2>
      {fatal ? (
        <div role="alert" className="error-banner">
          {fatal}
        </div>
      ) : null}

      <label className="field">
        <span>名称</span>
        <input
          value={form.name}
          onChange={(e) => update("name", e.target.value)}
          aria-invalid={!!errors.name}
        />
        {errors.name ? <em className="field-error">{errors.name}</em> : null}
      </label>

      <fieldset className="field">
        <legend>连接方式</legend>
        <label>
          <input
            type="radio"
            checked={form.connectionType === "direct_url"}
            onChange={() => update("connectionType", "direct_url")}
          />
          Direct URL
        </label>
        <label>
          <input
            type="radio"
            checked={form.connectionType === "ssh_forward"}
            onChange={() => update("connectionType", "ssh_forward")}
          />
          SSH Forward
        </label>
      </fieldset>

      {form.connectionType === "direct_url" ? (
        <DirectFields form={form} update={update} errors={errors} />
      ) : (
        <SshFields form={form} update={update} errors={errors} />
      )}

      <label className="field">
        <span>Pi Hub 密码（可选，用于 HTTP Basic 认证）</span>
        <input
          type="password"
          value={form.piHubPassword}
          onChange={(e) => update("piHubPassword", e.target.value)}
          autoComplete="off"
        />
      </label>

      <div className="form-actions">
        <button
          type="button"
          onClick={() => {
            void navigate("/");
          }}
        >
          取消
        </button>
        <button type="submit" disabled={submitting}>
          {submitting ? "保存中…" : "保存"}
        </button>
      </div>
    </form>
  );
}

function DirectFields({
  form,
  update,
  errors,
}: {
  form: FormState;
  update: <K extends keyof FormState>(key: K, value: FormState[K]) => void;
  errors: Record<string, string>;
}) {
  return (
    <label className="field">
      <span>URL</span>
      <input
        value={form.baseUrl}
        onChange={(e) => update("baseUrl", e.target.value)}
        placeholder="https://pi.example.com"
        aria-invalid={!!errors.baseUrl}
      />
      {errors.baseUrl ? (
        <em className="field-error">{errors.baseUrl}</em>
      ) : null}
      {isPlaintextDirectUrl(form.baseUrl) ? (
        <em className="field-warning">
          HTTP 为明文传输，连接前会再次提示确认（FR-005）。
        </em>
      ) : null}
    </label>
  );
}

function SshFields({
  form,
  update,
  errors,
}: {
  form: FormState;
  update: <K extends keyof FormState>(key: K, value: FormState[K]) => void;
  errors: Record<string, string>;
}) {
  return (
    <>
      <label className="field">
        <span>SSH Host</span>
        <input
          value={form.sshHost}
          onChange={(e) => update("sshHost", e.target.value)}
          aria-invalid={!!errors.sshHost}
        />
        {errors.sshHost ? (
          <em className="field-error">{errors.sshHost}</em>
        ) : null}
      </label>
      <label className="field">
        <span>SSH Port</span>
        <input
          type="number"
          value={form.sshPort}
          onChange={(e) => update("sshPort", Number(e.target.value))}
          aria-invalid={!!errors.sshPort}
        />
        {errors.sshPort ? (
          <em className="field-error">{errors.sshPort}</em>
        ) : null}
      </label>
      <label className="field">
        <span>用户名</span>
        <input
          value={form.sshUsername}
          onChange={(e) => update("sshUsername", e.target.value)}
          aria-invalid={!!errors.sshUsername}
        />
        {errors.sshUsername ? (
          <em className="field-error">{errors.sshUsername}</em>
        ) : null}
      </label>
      <fieldset className="field">
        <legend>认证方式</legend>
        <label>
          <input
            type="radio"
            checked={form.sshAuthType === "password"}
            onChange={() => update("sshAuthType", "password")}
          />
          密码
        </label>
        <label>
          <input
            type="radio"
            checked={form.sshAuthType === "private_key"}
            onChange={() => update("sshAuthType", "private_key")}
          />
          私钥
        </label>
      </fieldset>
      {form.sshAuthType === "password" ? (
        <label className="field">
          <span>SSH 密码</span>
          <input
            type="password"
            value={form.sshPassword}
            onChange={(e) => update("sshPassword", e.target.value)}
            autoComplete="off"
            aria-invalid={!!errors.sshPassword}
          />
          {errors.sshPassword ? (
            <em className="field-error">{errors.sshPassword}</em>
          ) : null}
        </label>
      ) : (
        <>
          <label className="field">
            <span>OpenSSH 私钥</span>
            <textarea
              value={form.privateKey}
              onChange={(e) => update("privateKey", e.target.value)}
              rows={6}
              placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
              aria-invalid={!!errors.privateKey}
            />
            {errors.privateKey ? (
              <em className="field-error">{errors.privateKey}</em>
            ) : null}
          </label>
          <label className="field">
            <span>私钥 Passphrase（如已加密）</span>
            <input
              type="password"
              value={form.privateKeyPassphrase}
              onChange={(e) => update("privateKeyPassphrase", e.target.value)}
              autoComplete="off"
            />
          </label>
        </>
      )}

      <details className="advanced">
        <summary>高级设置（Target Host / Port / Scheme）</summary>
        <label className="field">
          <span>Target Host（SSH Server 视角的目标）</span>
          <input
            value={form.targetHost}
            onChange={(e) => update("targetHost", e.target.value)}
          />
        </label>
        <label className="field">
          <span>Target Port</span>
          <input
            type="number"
            value={form.targetPort}
            onChange={(e) => update("targetPort", Number(e.target.value))}
            aria-invalid={!!errors.targetPort}
          />
          {errors.targetPort ? (
            <em className="field-error">{errors.targetPort}</em>
          ) : null}
        </label>
        <label className="field">
          <span>Service Scheme</span>
          <select
            value={form.serviceScheme}
            onChange={(e) =>
              update("serviceScheme", e.target.value as ServiceScheme)
            }
          >
            <option value="http">http</option>
            <option value="https">https</option>
          </select>
        </label>
        <label className="field">
          <span>Base Path</span>
          <input
            value={form.serviceBasePath}
            onChange={(e) => update("serviceBasePath", e.target.value)}
          />
        </label>
      </details>
    </>
  );
}

/** Build the DTO and persist any entered secrets to Keychain first. */
async function buildDraft(form: FormState): Promise<ProfileDraft> {
  // Pi Hub password is optional; only persist + reference if entered.
  let piHubCredentialId: string | null = null;
  if (form.piHubPassword) {
    piHubCredentialId = generateCredentialId();
    await putCredential(
      piHubCredentialId,
      "pi-hub-password",
      form.piHubPassword,
    );
  }

  if (form.connectionType === "direct_url") {
    return {
      connection_type: "direct_url",
      name: form.name.trim(),
      base_url: form.baseUrl.trim(),
      pi_hub_credential_id: piHubCredentialId,
    };
  }

  const sshCredentialId = generateCredentialId();
  if (form.sshAuthType === "password") {
    await putCredential(sshCredentialId, "ssh-password", form.sshPassword);
  } else {
    await putCredential(sshCredentialId, "ssh-private-key", form.privateKey);
    if (form.privateKeyPassphrase) {
      await putCredential(
        sshCredentialId,
        "ssh-key-passphrase",
        form.privateKeyPassphrase,
      );
    }
  }

  return {
    connection_type: "ssh_forward",
    name: form.name.trim(),
    ssh_host: form.sshHost.trim(),
    ssh_port: form.sshPort,
    ssh_username: form.sshUsername.trim(),
    ssh_auth_type: form.sshAuthType satisfies SshAuthType,
    ssh_credential_id: sshCredentialId,
    target_host: form.targetHost.trim() || "127.0.0.1",
    target_port: form.targetPort,
    service_scheme: form.serviceScheme,
    service_base_path: form.serviceBasePath || "/",
    pi_hub_credential_id: piHubCredentialId,
  };
}

function profileToForm(profile: ServiceProfile): FormState {
  const base: FormState = { ...DEFAULTS, name: profile.metadata.name };
  if (profile.connection_type === "direct_url") {
    return {
      ...base,
      connectionType: "direct_url",
      baseUrl: profile.base_url,
    };
  }
  return {
    ...base,
    connectionType: "ssh_forward",
    sshHost: profile.ssh_host,
    sshPort: profile.ssh_port,
    sshUsername: profile.ssh_username,
    sshAuthType: profile.ssh_auth_type,
    targetHost: profile.target_host,
    targetPort: profile.target_port,
    serviceScheme: profile.service_scheme,
    serviceBasePath: profile.service_base_path,
  };
}

function toMessage(e: unknown): string {
  const dto = e as AppErrorDto | undefined;
  return dto?.message ?? "发生未知错误。";
}

export type { CredentialKind };
