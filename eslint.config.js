import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

// ESLint flat config. Per AGENTS.md §10, TypeScript runs in strict mode and
// `any` must not be used to bypass types without a local justification.
export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "coverage/**",
      "src-tauri/**",
    ],
  },

  // Trusted App Shell source: full type-checked linting.
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      ...tseslint.configs.recommendedTypeChecked,
      ...tseslint.configs.stylisticTypeChecked,
    ],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        project: ["./tsconfig.json"],
        tsconfigDirName: import.meta.dirname,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
      // AGENTS.md §10: no `any` without a local justification.
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },

  // Test files: type-checked but allow assertion helpers.
  {
    files: ["src/**/*.{test,spec}.{ts,tsx}", "src/test-setup.ts"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: { ...globals.browser, ...globals.node },
      parserOptions: {
        project: ["./tsconfig.json"],
        tsconfigDirName: import.meta.dirname,
      },
    },
    rules: {
      "@typescript-eslint/no-non-null-assertion": "off",
    },
  },

  // Root config files (vite/vitest): lighter, non-type-checked linting so the
  // project does not need a separate tsconfig project entry for them.
  {
    files: ["vite.config.ts", "vitest.config.ts", "eslint.config.js"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      globals: { ...globals.node },
    },
  },
);
