import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { deleteService, listServices } from "./api";
import type { ServiceProfile } from "./model";
import { connectionStateLabel } from "../connection/model";
import { LocalRuntimeCard } from "../local-runtime/LocalRuntimeCard";
import { getLocalRuntimePlatformSupport } from "../local-runtime/api";
import { PagePlaceholder } from "../../components/PagePlaceholder";
import type { AppErrorDto } from "../../lib/tauri";

/**
 * Service list — the V1 home page (docs/requirements-v1.md §8.1, FR-001).
 *
 * Lists saved services with name, connection type and last status. Status is
 * never fabricated as "Online"; default is "未连接" (FR-001). Delete is a
 * destructive action requiring confirmation (FR-004).
 */
export function ServiceListPage() {
  const [services, setServices] = useState<ServiceProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<ServiceProfile | null>(
    null,
  );
  const [deleting, setDeleting] = useState(false);
  const [localRuntimeSupported, setLocalRuntimeSupported] = useState(false);
  const navigate = useNavigate();

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setServices(await listServices());
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
    // V2: hide the local runtime card on iOS (requirements-v2 §4.2).
    void getLocalRuntimePlatformSupport().then(setLocalRuntimeSupported);
  }, [reload]);

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setDeleting(true);
    try {
      await deleteService(pendingDelete.metadata.id);
      setPendingDelete(null);
      await reload();
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setDeleting(false);
    }
  };

  if (loading) {
    return (
      <PagePlaceholder title="加载服务列表…">
        <p>正在读取本地服务配置。</p>
      </PagePlaceholder>
    );
  }

  return (
    <div className="home-page">
      {localRuntimeSupported ? <LocalRuntimeCard /> : null}
      {error ? (
        <div role="alert" className="error-banner">
          {error}
        </div>
      ) : null}
      {services.length === 0 ? (
        <span className="sr-only">还没有 Pi Hub 服务</span>
      ) : null}
      {services.length > 0 ? (
        <ul className="service-list" aria-label="服务列表">
          {services.map((service) => (
            <li key={service.metadata.id}>
              <Link
                to={`/connect/${service.metadata.id}`}
                className="service-row"
              >
                <span className="name">{service.metadata.name}</span>
                <span className="meta">
                  {service.connection_type === "direct_url"
                    ? "Direct URL"
                    : "SSH Forward"}{" "}
                  · {connectionStateLabel("idle")}
                </span>
              </Link>
              <div className="service-actions">
                <button
                  type="button"
                  onClick={() => {
                    void navigate(`/connect/${service.metadata.id}`);
                  }}
                >
                  连接
                </button>
                <Link to={`/services/${service.metadata.id}/edit`}>编辑</Link>
                <button
                  type="button"
                  className="danger"
                  onClick={() => setPendingDelete(service)}
                >
                  删除
                </button>
              </div>
            </li>
          ))}
        </ul>
      ) : null}
      <Link className="add-button" to="/services/new">
        + 添加服务
      </Link>

      {pendingDelete ? (
        <div className="modal-backdrop" role="dialog" aria-modal="true">
          <div className="modal">
            <h3>删除服务？</h3>
            <p>
              将删除「{pendingDelete.metadata.name}
              」并断开其活动连接，同时清理不再被引用的凭据与 Host Key
              记录。此操作不可撤销。
            </p>
            <div className="modal-actions">
              <button
                type="button"
                disabled={deleting}
                onClick={() => setPendingDelete(null)}
              >
                取消
              </button>
              <button
                type="button"
                className="danger"
                disabled={deleting}
                onClick={() => void confirmDelete()}
              >
                {deleting ? "删除中…" : "确认删除"}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function toMessage(e: unknown): string {
  const dto = e as AppErrorDto | undefined;
  return dto?.message ?? "发生未知错误。";
}
