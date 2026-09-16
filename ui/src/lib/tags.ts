import type { TagCount, TagRule } from './api';

/** What committing a rename would do. `same` needs no call; `merge` asks first, because
 *  two tags become one and only restoring the rule separates them again. Names compare
 *  exactly, as the backend does. */
export type RenameCheck = 'blank' | 'same' | 'merge' | 'ok';

export function renameCheck(from: string, to: string, existing: readonly TagCount[]): RenameCheck {
  const name = to.trim();
  if (name === '') return 'blank';
  if (name === from) return 'same';
  return existing.some((t) => t.tag === name) ? 'merge' : 'ok';
}

/** The Settings list's filter: a case-insensitive substring. Done here rather than in the
 *  backend because the whole list is already on screen. */
export function filterTags(tags: readonly TagCount[], query: string): TagCount[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') return [...tags];
  return tags.filter((t) => t.tag.toLowerCase().includes(needle));
}

export function ruleLabel(rule: TagRule): string {
  return rule.target === null ? `${rule.tag} — removed` : `${rule.tag} → ${rule.target}`;
}
