import { Route, Routes } from "react-router-dom";
import { ServiceListPage } from "../features/services/ServiceListPage";
import { ServiceFormPage } from "../features/services/ServiceFormPage";
import { ConnectionPage } from "../features/connection/ConnectionPage";
import { ViewerToolbar } from "../features/viewer/ViewerToolbar";
import { PagePlaceholder } from "../components/PagePlaceholder";

/**
 * Application route tree.
 *
 * Routes mirror the core V1 flow
 * (open app → pick a service → connect → open Pi Hub). Individual pages are
 * placeholders during project initialization; their implementations land in
 * the corresponding V1 phases.
 */
export function AppRoutes() {
  return (
    <Routes>
      <Route path="/" element={<ServiceListPage />} />
      <Route path="/services/new" element={<ServiceFormPage mode="create" />} />
      <Route
        path="/services/:id/edit"
        element={<ServiceFormPage mode="edit" />}
      />
      <Route path="/connect/:id" element={<ConnectionPage />} />
      <Route path="/viewer/:id" element={<ViewerToolbar />} />
      <Route
        path="*"
        element={
          <PagePlaceholder title="Page not found">
            <p>The page you requested does not exist.</p>
          </PagePlaceholder>
        }
      />
    </Routes>
  );
}
