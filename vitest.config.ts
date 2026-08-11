import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Force React to load its development build in tests so `react.act` is
  // available (testing-library v16 selects `react.act` when it is a function).
  // React's entry dispatches on `process.env.NODE_ENV === "production"`; the
  // production build omits the `act` export, which made
  // `react-dom/test-utils.act` crash with "React.act is not a function".
  define: {
    "process.env.NODE_ENV": JSON.stringify("test"),
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.{test,spec}.{ts,tsx}",
        "src/test-setup.ts",
        "src/main.tsx",
      ],
    },
  },
});
