import { extensionOf } from "./utils";

/**
 * The editor's own copy of prettier, for "Format document" in a project that
 * has none installed (the backend prefers the project's). Loaded the first
 * time it is needed: nothing here is in the app's start-up bundle.
 */

type Parser = "babel" | "typescript" | "json" | "json5" | "css" | "scss" | "less" | "html" | "vue" | "markdown" | "mdx" | "yaml" | "graphql";

const PARSER_BY_EXT: Record<string, Parser> = {
  js: "babel",
  jsx: "babel",
  mjs: "babel",
  cjs: "babel",
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  json: "json",
  jsonc: "json",
  json5: "json5",
  css: "css",
  scss: "scss",
  less: "less",
  html: "html",
  htm: "html",
  vue: "vue",
  md: "markdown",
  markdown: "markdown",
  mdx: "mdx",
  yaml: "yaml",
  yml: "yaml",
  graphql: "graphql",
  gql: "graphql",
};

/** Whether the bundled prettier can format a file of this name. */
export function bundledParser(name: string): Parser | null {
  return PARSER_BY_EXT[extensionOf(name)] ?? null;
}

function plugins(parser: Parser) {
  const estree = () => import("prettier/plugins/estree");
  const babel = () => import("prettier/plugins/babel");
  const postcss = () => import("prettier/plugins/postcss");
  switch (parser) {
    case "babel":
    case "json":
    case "json5":
      return [babel(), estree()];
    case "typescript":
      return [import("prettier/plugins/typescript"), estree()];
    case "css":
    case "scss":
    case "less":
      return [postcss()];
    // Markup formats their embedded scripts and styles too.
    case "html":
    case "vue":
      return [import("prettier/plugins/html"), babel(), estree(), postcss()];
    case "markdown":
    case "mdx":
      return [import("prettier/plugins/markdown"), babel(), estree(), postcss(), import("prettier/plugins/yaml")];
    case "yaml":
      return [import("prettier/plugins/yaml")];
    case "graphql":
      return [import("prettier/plugins/graphql")];
  }
}

/** The project's settings when they are JSON (`.prettierrc`, package.json); anything else is ignored. */
function options(config: string | null): Record<string, unknown> {
  if (!config) return {};
  try {
    const parsed: unknown = JSON.parse(config);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    // Plugins cannot be loaded here, and overrides need globbing prettier does itself.
    const { plugins: _plugins, overrides: _overrides, ...rest } = parsed as Record<string, unknown>;
    return rest;
  } catch {
    return {};
  }
}

/** Formats `text` as a file called `name`, or returns null when prettier has no parser for it. */
export async function formatWithBundledPrettier(name: string, text: string, config: string | null): Promise<string | null> {
  const parser = bundledParser(name);
  if (!parser) return null;
  const [{ format }, ...loaded] = await Promise.all([import("prettier/standalone"), ...plugins(parser)]);
  return format(text, { ...options(config), parser, filepath: name, plugins: loaded });
}
