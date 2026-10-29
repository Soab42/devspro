// Builds site/assets/icons.svg from the lucide icons the app already depends
// on (lucide-react, ISC). To add an icon: put its lucide name in the list
// below and run, from the repo root,
//
//   node site/press/make-icons.mjs
//
// then use it as <svg class="i"><use href="assets/icons.svg#name"/></svg>.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const iconsDir = join(root, "node_modules/lucide-react/dist/esm/icons");
const out = join(root, "site/assets/icons.svg");
const names = `layout-dashboard globe container database table-2 mail workflow calendar-clock database-zap network memory-stick eraser scroll-text send app-window package files file-diff book-user square-terminal bot terminal folder-sync screen-share settings download shield-check lock refresh-cw check arrow-right copy chevron-down rocket badge-check monitor circle-check cpu hard-drive gauge activity search file-signature ban layers`.split(" ");
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/"/g, "&quot;");
let symbols = [];
for (const n of names) {
  let file = `${n}.mjs`;
  // Renamed icons are one-line re-exports of their new file.
  for (;;) {
    const src = readFileSync(`${iconsDir}/${file}`, "utf8");
    const m = src.match(/export \{ default \} from '\.\/(.+?)';/);
    if (!m) break;
    file = m[1];
  }
  const mod = await import(`${iconsDir}/${file}`);
  const inner = mod.__iconNode
    .map(([tag, attrs]) => {
      const a = Object.entries(attrs).filter(([k]) => k !== "key").map(([k, v]) => `${k}="${esc(v)}"`).join(" ");
      return `<${tag} ${a}/>`;
    })
    .join("");
  symbols.push(`  <symbol id="${n}" viewBox="0 0 24 24">${inner}</symbol>`);
}
// GitHub mark (Octicons, MIT) — lucide v1 dropped brand icons.
symbols.push(`  <symbol id="github" viewBox="0 0 16 16"><path fill="currentColor" stroke="none" d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/></symbol>`);
const svg = `<svg xmlns="http://www.w3.org/2000/svg" style="display:none">
<!-- Icons from Lucide (ISC License, https://lucide.dev), the set the DevsPro app uses.
     GitHub mark from Octicons (MIT). Regenerate rather than hand-edit. -->
<defs>
${symbols.join("\n")}
</defs>
</svg>
`;
writeFileSync(out, svg);
console.log(`${names.length + 1} icons -> site/assets/icons.svg`);
