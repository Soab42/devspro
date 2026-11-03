import { describe, expect, it } from "vitest";
import { composeFragment, snippets } from "./ConnectTab";

describe("Docker snippets", () => {
  it("turns the environment into a compose service that can reach the host", () => {
    const env = snippets("http://host.docker.internal:4318").langsmith.env;
    expect(composeFragment(env)).toBe(
      [
        "services:",
        "  your-app:",
        "    environment:",
        '      LANGSMITH_TRACING: "true"',
        '      LANGSMITH_ENDPOINT: "http://host.docker.internal:4318"',
        '      LANGSMITH_API_KEY: "local"',
        '      LANGSMITH_PROJECT: "my-app"',
        "    extra_hosts:",
        '      - "host.docker.internal:host-gateway"',
      ].join("\n"),
    );
  });

  it("points every SDK at the address it is given", () => {
    for (const snippet of Object.values(snippets("http://host.docker.internal:9999"))) {
      expect(snippet.env).toContain("http://host.docker.internal:9999");
      expect(snippet.env).not.toContain("localhost");
    }
  });
});
