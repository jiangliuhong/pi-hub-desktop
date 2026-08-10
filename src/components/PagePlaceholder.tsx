import type { ReactNode } from "react";

/**
 * Shared placeholder surface for pages that have not been implemented yet.
 *
 * During project initialization every route renders this. As each V1 phase
 * lands, placeholders are replaced with real feature pages.
 */
export function PagePlaceholder({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <section className="placeholder">
      <h2>{title}</h2>
      {children}
    </section>
  );
}
