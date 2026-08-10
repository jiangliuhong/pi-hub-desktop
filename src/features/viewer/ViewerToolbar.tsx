import { useParams } from "react-router-dom";
import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Trusted toolbar over the untrusted Service View
 * (docs/requirements-v1.md §8.5).
 *
 * Provides return-to-list, status, refresh and disconnect controls. These
 * controls are App Shell UI only — the remote Pi Hub page cannot reach them.
 * Implementation lands after the Phase 0 Service View spike.
 */
export function ViewerToolbar() {
  const { id } = useParams<{ id: string }>();
  return (
    <PagePlaceholder title="Pi Hub">
      <p>
        服务 {id} 的受控 WebView 工具栏与 Pi Hub WebUI
        集成将在技术验证完成后实现。
      </p>
    </PagePlaceholder>
  );
}
