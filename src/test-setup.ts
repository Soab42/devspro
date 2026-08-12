/**
 * jsdom gaps that the app legitimately relies on.
 *
 * matchMedia: ThemeContext resolves "system" in JS (rather than in a CSS media
 * query) so that dark mode can be a single `:root[data-theme="dark"]` block
 * instead of a duplicated palette. jsdom doesn't implement it.
 */
if (!window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

// Used by the copy affordances on Mono and CopyButton.
if (!navigator.clipboard) {
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: async () => {} },
    configurable: true,
  });
}
