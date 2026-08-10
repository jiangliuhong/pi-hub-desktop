import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { listServices } from "./api";
import type { ServiceProfile } from "./model";
import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Service list — the V1 home page (docs/requirements-v1.md §8.1).
 *
 * During initialization this renders the empty state and the "add service"
 * entry point. Listing, status and "last connection" display arrive in
 * Phase 1; the page must never fabricate an "Online" status (FR-001).
 */
export function ServiceListPage() {
  const [services, setServices] = useState<ServiceProfile[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const result = await listServices();
        if (!cancelled) setServices(result);
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div>
      {loading ? (
        <PagePlaceholder title="加载服务列表…">
          <p>正在读取本地服务配置。</p>
        </PagePlaceholder>
      ) : services.length === 0 ? (
        <PagePlaceholder title="还没有 Pi Hub 服务">
          <p>添加你的第一个 Pi Hub 服务以开始使用。</p>
          <Link className="add-button" to="/services/new">
            + 添加服务
          </Link>
          <p className="phase-note">
            服务列表、Direct URL 与 SSH Forward 的完整实现将在 V1 Phase 1 /
            Phase 2 完成。
          </p>
        </PagePlaceholder>
      ) : (
        <ul className="service-list">
          {services.map((service) => (
            <li key={service.metadata.id}>
              <Link to={`/connect/${service.metadata.id}`}>
                <span className="name">{service.metadata.name}</span>
              </Link>
              <span className="meta">
                {service.connection_type === "direct_url"
                  ? "Direct URL"
                  : "SSH Forward"}{" "}
                · 未连接
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
