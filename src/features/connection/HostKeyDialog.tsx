import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Host Key confirmation surface (docs/requirements-v1.md FR-007 / FR-008).
 *
 * First connection must show host, port, algorithm and SHA-256 fingerprint and
 * require explicit user confirmation. A changed key must block — never silently
 * overwrite (AGENTS.md §6.2). Implementation lands in V1 Phase 2.
 */
export function HostKeyDialog() {
  return (
    <PagePlaceholder title="SSH Host Key 确认">
      <p>
        Host Key 算法与 SHA-256 Fingerprint 展示、确认与替换流程将在 V1 Phase 2
        实现。
      </p>
    </PagePlaceholder>
  );
}
