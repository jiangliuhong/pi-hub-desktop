import { useParams } from "react-router-dom";
import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Connection progress / diagnostics page (docs/requirements-v1.md §8.4).
 *
 * Drives the `connect_service` flow and renders the SSH Host Key challenge and
 * retry / cancel affordances. Implementation lands in V1 Phase 2; this is a
 * routed placeholder so the navigation structure exists.
 */
export function ConnectionPage() {
  const { id } = useParams<{ id: string }>();
  return (
    <PagePlaceholder title="正在连接…">
      <p>
        服务 {id} 的连接流程（状态、Host Key 确认、诊断与重试）将在 V1 Phase 2
        实现。
      </p>
    </PagePlaceholder>
  );
}
