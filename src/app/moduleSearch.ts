import type { ModuleDefinition } from "./ModuleRegistry";

/**
 * Ranking for the command palette's search box.
 *
 * Three match tiers, strongest to weakest: a field that STARTS WITH the query
 * word, a field that CONTAINS it anywhere, and a field whose letters contain
 * the word's letters in order but not necessarily adjacent (so "dkr" finds
 * "Docker"). Label matches always outrank keyword matches, which always
 * outrank description matches — the label is what the user reads, a keyword
 * is a synonym someone typed on purpose, and the description is prose that
 * happens to mention the word.
 */
const FIELD_WEIGHT = {
  label: 100,
  keyword: 10,
  description: 1,
} as const;

const TIER_SCORE = {
  prefix: 3,
  substring: 2,
  subsequence: 1,
} as const;

/** True when every letter of `word`, in order, appears somewhere in `text`. */
function isSubsequence(word: string, text: string): boolean {
  let i = 0;
  for (let j = 0; j < text.length && i < word.length; j++) {
    if (text[j] === word[i]) i++;
  }
  return i === word.length;
}

/**
 * 0 if `word` (already lowercase) doesn't match `text` at all.
 *
 * `allowSubsequence` is only turned on for the label. Scattered-letter
 * matching against a whole sentence of keywords or prose throws up noise — a
 * short word like "cron" is a subsequence of almost any long-enough
 * paragraph — whereas a module's label is short enough that it stays a
 * genuinely fuzzy shorthand, which is the whole point of "dkr" finding
 * "Docker".
 */
function tierScore(word: string, text: string, allowSubsequence: boolean): number {
  if (!text) return 0;
  const lowerText = text.toLowerCase();
  if (lowerText.startsWith(word)) return TIER_SCORE.prefix;
  if (lowerText.includes(word)) return TIER_SCORE.substring;
  return allowSubsequence && isSubsequence(word, lowerText) ? TIER_SCORE.subsequence : 0;
}

/** Best weighted tier for one query word against one module, 0 if it matches nowhere on it. */
function wordScore(word: string, m: ModuleDefinition): number {
  const label = tierScore(word, m.label, true) * FIELD_WEIGHT.label;
  const keyword = (m.keywords ?? []).reduce(
    (best, k) => Math.max(best, tierScore(word, k, false) * FIELD_WEIGHT.keyword),
    0,
  );
  const description = tierScore(word, m.description ?? "", false) * FIELD_WEIGHT.description;
  return Math.max(label, keyword, description);
}

/**
 * Ranks `candidates` against `query`.
 *
 * An empty (or whitespace-only) query returns `candidates` untouched, in
 * registry order — the launcher's default list before the user has typed
 * anything. Otherwise every word in the query must match somewhere on a
 * candidate — in any order, in any field — or that candidate is dropped;
 * survivors are sorted by the sum of their best per-word score, descending.
 * `Array#sort` is stable, so ties keep the candidates' original order.
 */
export function rankModules(query: string, candidates: ModuleDefinition[]): ModuleDefinition[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return candidates;

  const scored: { module: ModuleDefinition; score: number }[] = [];
  for (const module of candidates) {
    let total = 0;
    let matchedAll = true;
    for (const word of words) {
      const score = wordScore(word, module);
      if (score === 0) {
        matchedAll = false;
        break;
      }
      total += score;
    }
    if (matchedAll) scored.push({ module, score: total });
  }

  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.module);
}
