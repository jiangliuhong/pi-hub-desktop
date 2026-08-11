import "@testing-library/jest-dom/vitest";

// React 19 moved `act` into the `react` package; testing-library needs the act
// environment flag set so it uses `react.act` rather than the deprecated
// `react-dom/test-utils` act (which fails with "React.act is not a function").
(
  globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
