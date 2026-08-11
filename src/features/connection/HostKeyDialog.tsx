import type { HostKeyChallengeDto } from "./api";

/**
 * Host Key confirmation surface (docs/requirements-v1.md FR-007 / FR-008).
 *
 * First connection must show host, port, algorithm and SHA-256 fingerprint and
 * require explicit user confirmation. A changed key is blocked upstream by the
 * Rust core and never reaches this "first-time confirm" dialog; the separate
 * replace flow shows old + new fingerprints (FR-008).
 */
export interface HostKeyDialogProps {
  challenge: HostKeyChallengeDto;
  onRespond: (accept: boolean) => void;
}

export function HostKeyDialog({ challenge, onRespond }: HostKeyDialogProps) {
  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal host-key">
        <h3>首次连接，请确认 SSH Host Key</h3>
        <p className="host-line">
          <strong>
            {challenge.ssh_host}:{challenge.ssh_port}
          </strong>
        </p>
        <dl className="host-key-facts">
          <dt>算法</dt>
          <dd>{challenge.algorithm}</dd>
          <dt>SHA-256 Fingerprint</dt>
          <dd>
            <code>{challenge.sha256_fingerprint}</code>
          </dd>
        </dl>
        <p className="hint">
          确认前请与服务器可信来源（控制台、文档）核对该 Fingerprint。
          确认后该公钥将绑定到此 host:port；后续连接若 Key 变化将自动阻断。
        </p>
        <div className="modal-actions">
          <button type="button" onClick={() => onRespond(false)}>
            拒绝
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => onRespond(true)}
          >
            确认并连接
          </button>
        </div>
      </div>
    </div>
  );
}
