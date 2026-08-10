import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Service create/edit form (docs/requirements-v1.md §8.2).
 *
 * The form must render fields conditionally on `connection_type` and validate
 * before save (FR-002, FR-003, SR-007). Implementation lands in V1 Phase 1;
 * for now this is a routed placeholder so the navigation structure exists.
 */
export function ServiceFormPage({ mode }: { mode: "create" | "edit" }) {
  return (
    <PagePlaceholder title={mode === "create" ? "添加服务" : "编辑服务"}>
      <p>
        服务表单（Direct URL / SSH Forward 的条件字段、校验与 Keychain
        凭据录入） 将在 V1 Phase 1 实现。
      </p>
    </PagePlaceholder>
  );
}
