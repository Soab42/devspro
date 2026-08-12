import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    // Most suites are pure functions and run fine in node, but the component
    // guards (Page's single <h1>, useResource's polling behaviour) need a DOM.
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/test-setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // Vitest stubs CSS modules to empty strings by default, which silently made
    // the stylesheet guards in ui/conventions.test.ts and ui/visibility.test.tsx
    // assert against "" and pass no matter what the CSS said.
    css: true,
  },
});
