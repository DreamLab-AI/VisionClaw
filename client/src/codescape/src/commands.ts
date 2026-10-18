/**
 * The view half of the command grammar (codescape-collab `Command`): commands
 * that change only this client's view are applied locally at once and still
 * forwarded to the server so presence/timeline record them. Graph queries
 * (focus, callers, affected, path, explain, ask…) go to the server.
 */
import type { Lang } from './protocol';

export type LocalCommand =
  | { kind: 'overview' }
  | { kind: 'zoom_in' }
  | { kind: 'zoom_out' }
  | { kind: 'back' }
  | { kind: 'show_all' }
  | { kind: 'hide_tests' }
  | { kind: 'show_only'; lang: Lang | null; community: number | null }
  | { kind: 'isolate'; community: number | null }
  | { kind: 'rewind'; seconds: number }
  | { kind: 'replay' }
  | { kind: 'live' }
  | { kind: 'pause' }
  | { kind: 'follow'; actor: string }
  | { kind: 'stop_following' }
  | { kind: 'pick'; n: number }
  | { kind: 'clear' }
  | { kind: 'help' };

const NUMBER_WORDS: Record<string, number> = {
  zero: 0, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9, ten: 10,
  eleven: 11, twelve: 12, fifteen: 15, twenty: 20, thirty: 30, forty: 40, fifty: 50, sixty: 60, ninety: 90,
  first: 1, second: 2, third: 3, fourth: 4, fifth: 5,
};

const LANGS: Record<string, Lang> = {
  rust: 'rust', rs: 'rust', typescript: 'type_script', ts: 'type_script', javascript: 'java_script', js: 'java_script',
  python: 'python', py: 'python', go: 'go', golang: 'go', gdscript: 'gd_script', godot: 'gd_script', cpp: 'cpp', 'c++': 'cpp',
  cuda: 'cuda', shell: 'shell', bash: 'shell', nix: 'nix', markdown: 'markdown', docs: 'markdown',
};

export function langFromWord(w: string): Lang | null {
  return LANGS[w.toLowerCase()] ?? null;
}

/** Lower-case, strip fillers and punctuation, number words → digits (mirrors `normalise_spoken`). */
export function normalise(text: string): string {
  return text
    .toLowerCase()
    .replace(/[?!,]/g, ' ')
    .replace(/\b(um+|uh+|please|can you|could you|would you|just|hey codescape)\b/g, ' ')
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => (w in NUMBER_WORDS ? String(NUMBER_WORDS[w]) : w))
    .join(' ')
    .trim();
}

export function parseLocalCommand(raw: string): LocalCommand | null {
  const t = normalise(raw);
  if (!t) return null;
  if (/^(overview|zoom out all the way|show everything|reset view|home)$/.test(t)) return { kind: 'overview' };
  if (/^zoom in$/.test(t)) return { kind: 'zoom_in' };
  if (/^zoom out$/.test(t)) return { kind: 'zoom_out' };
  if (/^(back|go back|previous)$/.test(t)) return { kind: 'back' };
  if (/^(show all|clear filters?|reset filters?)$/.test(t)) return { kind: 'show_all' };
  if (/^hide tests?$/.test(t)) return { kind: 'hide_tests' };
  if (/^(clear|clear selection|deselect|escape)$/.test(t)) return { kind: 'clear' };
  if (/^(help|\?)$/.test(t)) return { kind: 'help' };
  let m = /^(?:number|pick|option|choose) (\d+)$/.exec(t) ?? /^(\d+)$/.exec(t);
  if (m) return { kind: 'pick', n: Number(m[1]) };
  m = /^show only (.+)$/.exec(t);
  if (m) {
    const c = /^community (\d+)$/.exec(m[1]);
    if (c) return { kind: 'show_only', lang: null, community: Number(c[1]) };
    const lang = langFromWord(m[1].replace(/ (code|files)$/, ''));
    if (lang) return { kind: 'show_only', lang, community: null };
    return null;
  }
  m = /^isolate(?: community)?(?: (\d+))?$/.exec(t);
  if (m) return { kind: 'isolate', community: m[1] !== undefined ? Number(m[1]) : null };
  m = /^(?:rewind|go back) (\d+)\s*(s|sec|secs|seconds?|m|min|mins|minutes?)?$/.exec(t);
  if (m) {
    const unit = m[2] ?? 's';
    return { kind: 'rewind', seconds: Number(m[1]) * (unit.startsWith('m') ? 60 : 1) };
  }
  if (/^replay$/.test(t)) return { kind: 'replay' };
  if (/^(live|go live|resume)$/.test(t)) return { kind: 'live' };
  if (/^(pause|freeze)$/.test(t)) return { kind: 'pause' };
  if (/^(stop following|unfollow)$/.test(t)) return { kind: 'stop_following' };
  m = /^follow (.+)$/.exec(t);
  if (m) return { kind: 'follow', actor: m[1] };
  return null;
}

export const HELP_TEXT = [
  'focus <name> · find <name> · explain <name>',
  'callers of <name> [depth N] · callees of <name> · affected by <name>',
  'path from <a> to <b> · number N (pick a candidate)',
  'show only rust|typescript|… · show only community N · isolate [N] · hide tests · show all',
  'zoom in · zoom out · overview · back · clear',
  'rewind 30 seconds · replay · pause · live',
  'what is claude doing · follow <actor> · stop following · ask <actor> to <task>',
].join('\n');

/**
 * Split a server-formatted candidate label (`name — path:line`) at the first
 * ` — ` so the name can stay prominent and the location be dimmed.
 * Labels without the separator come back whole with an empty `detail`.
 */
export function splitCandidate(label: string): { name: string; detail: string } {
  const sep = ' \u2014 ';
  const i = label.indexOf(sep);
  return i < 0 ? { name: label, detail: '' } : { name: label.slice(0, i), detail: label.slice(i) };
}
