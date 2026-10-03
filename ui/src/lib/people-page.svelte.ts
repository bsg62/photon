/** The People page's behaviour, apart from its markup so it can be tested: the sections as
 *  the backend last answered, the faces "Show more" has loaded on top, the selection, and
 *  the faces and groups hidden optimistically while an action is in flight. */

import type { FaceFilter, PageFace, PageGroup, PeoplePage } from './api';
import { nameChoice, type NameChoice } from './people';
import { singleFlight } from './single-flight';

/** Faces a strip shows before "Show more". */
export const STRIP = 12;
/** Faces one "Show more" asks for, and one page of a reload's re-fetch: the most one
 *  `person_faces` call hands over (`MAX_FACE_PAGE` in `library/people.rs`). Asking for more
 *  gets this many, so a re-fetch asked for in one call folded a strip back to it. */
export const MORE = 200;

export type Section = 'unnamed' | 'suggestion' | 'person' | 'ignored';
export type StripKey = string;
export type FaceAction = 'confirm' | 'reject' | 'ignore' | 'unignore';
export const SINGLE: StripKey = 'single';
export const IGNORED_FACES: StripKey = 'ignored-faces';
export const stripKey = (section: Section, id: number): StripKey => `${section}:${id}`;

export interface PeoplePageDeps {
  load(strip: number): Promise<PeoplePage>;
  more(person: number, which: FaceFilter, offset: number, limit: number): Promise<PageFace[]>;
  name(group: number, name: string): Promise<number>;
  rename(person: number, name: string): Promise<number>;
  confirm(faces: number[]): Promise<void>;
  reject(faces: number[]): Promise<void>;
  merge(from: number, into: number): Promise<void>;
  ignoreGroup(group: number, ignored: boolean): Promise<void>;
  ignoreFaces(faces: number[], ignored: boolean): Promise<void>;
  remove(person: number): Promise<void>;
  ask(message: string, title: string): Promise<boolean>;
  reportError(e: unknown): void;
}

const FILTER: Record<Section, FaceFilter> = {
  unnamed: 'all',
  suggestion: 'unconfirmed',
  person: 'confirmed',
  ignored: 'all',
};

const ACTIONS: Record<Section | 'single' | 'ignored-faces', FaceAction[]> = {
  unnamed: ['reject', 'ignore'],
  suggestion: ['confirm', 'reject', 'ignore'],
  person: ['reject', 'ignore'],
  ignored: [],
  single: ['ignore'],
  'ignored-faces': ['unignore'],
};

export function createPeoplePage(deps: PeoplePageDeps) {
  let page = $state.raw<PeoplePage | null>(null);
  /** Faces "Show more" loaded beyond each strip's first `STRIP`. */
  let extra = $state<Record<StripKey, PageFace[]>>({});
  let selection = $state<{ strip: StripKey; ids: number[] } | null>(null);
  /** Faces and groups hidden while an action on them is in flight, and after it until a
   *  reload that started after it lands: `null` while in flight, then the tick it ended at.
   *  A reload that started earlier may have read the backend before the write, and would
   *  otherwise put back what the user just removed. */
  let hiddenFaces = $state<Map<number, number | null>>(new Map());
  let hiddenGroups = $state<Map<number, number | null>>(new Map());
  let tick = 0;
  /** Moved by every "Show more" and "Show fewer" as it changes a strip's extra faces. A
   *  reload keeps its re-fetch of a strip only if the strip's number is where it was when the
   *  reload began: otherwise a click made while the re-fetch was in flight would be undone by
   *  a re-fetch of what the strip held before it. Not state: nothing draws it. */
  const generation: Record<StripKey, number> = {};
  const bump = (key: StripKey) => (generation[key] = (generation[key] ?? 0) + 1);

  const parse = (key: StripKey): { section: Section | 'single' | 'ignored-faces'; id: number } => {
    const [section, id] = key.split(':');
    return { section: section as Section | 'single' | 'ignored-faces', id: Number(id) };
  };

  function groupOf(key: StripKey, from: PeoplePage | null = page): PageGroup | undefined {
    if (!from) return undefined;
    const { section, id } = parse(key);
    const list =
      section === 'unnamed' ? from.unnamed
      : section === 'suggestion' ? from.suggestions
      : section === 'person' ? from.people
      : section === 'ignored' ? from.ignoredGroups
      : [];
    return list.find((g) => g.id === id);
  }

  function baseFaces(key: StripKey): PageFace[] {
    if (!page) return [];
    if (key === SINGLE) return page.singleFaces;
    if (key === IGNORED_FACES) return page.ignoredFaces;
    return groupOf(key)?.faces ?? [];
  }

  const shown = (f: PageFace) => !hiddenFaces.has(f.id);

  /** The strip's faces with what "Show more" loaded, each face once. The extra faces are a
   *  later read than the strip's first ones, and a face that moved between the two reads can
   *  be in both; listed twice, it would break the strip's keyed `{#each}`. */
  function listed(key: StripKey): PageFace[] {
    const seen = new Set<number>();
    return [...baseFaces(key), ...(extra[key] ?? [])].filter((f) => !seen.has(f.id) && !!seen.add(f.id));
  }

  function faces(key: StripKey): PageFace[] {
    return listed(key).filter(shown);
  }

  function count(key: StripKey): number {
    const all = listed(key);
    const gone = all.length - all.filter(shown).length;
    if (!page) return 0;
    if (key === SINGLE) return page.singleCount - gone;
    if (key === IGNORED_FACES) return page.ignoredFaces.length - gone;
    return (groupOf(key)?.faceCount ?? 0) - gone;
  }

  /** Drops what the reload that started at `started` has seen land, and the parts of the
   *  selection and of "Show more" that no longer exist. */
  function settle(started: number) {
    for (const map of [hiddenFaces, hiddenGroups])
      for (const [id, ended] of map) if (ended !== null && ended < started) map.delete(id);
    hiddenFaces = new Map(hiddenFaces);
    hiddenGroups = new Map(hiddenGroups);
    if (selection) {
      const present = new Set(faces(selection.strip).map((f) => f.id));
      const ids = selection.ids.filter((id) => present.has(id));
      selection = ids.length ? { strip: selection.strip, ids } : null;
    }
  }

  /** The `want` faces after a group's strip, a page of `MORE` at a time, until a short page
   *  says there are no more. */
  async function refetch(group: PageGroup, section: Section, want: number): Promise<PageFace[]> {
    const got: PageFace[] = [];
    while (got.length < want) {
      const limit = Math.min(MORE, want - got.length);
      const page = await deps.more(group.id, FILTER[section], group.faces.length + got.length, limit);
      got.push(...page);
      if (page.length < limit) break;
    }
    return got;
  }

  /** `next` in the order `shown` had them, the groups new to it after in the backend's
   *  order. The backend sorts Unnamed by size, and a reload during a scan or a grouping run
   *  can reorder groups whose counts moved; the keyed rows would then move under the user,
   *  blurring the name field being typed in and putting another group under the pointer
   *  between press and click. Only the first answer is taken as sorted. */
  function keepOrder(shown: PageGroup[] | undefined, next: PageGroup[]): PageGroup[] {
    if (!shown) return next;
    const at = new Map(shown.map((g, i) => [g.id, i]));
    const known = next.filter((g) => at.has(g.id)).sort((a, b) => at.get(a.id)! - at.get(b.id)!);
    return [...known, ...next.filter((g) => !at.has(g.id))];
  }

  async function fetch(): Promise<void> {
    const started = ++tick;
    const before = { ...generation };
    const next = await deps.load(STRIP);
    // "Show more" survives a reload: an action reloads the page, and a strip the user is
    // working through must not fold up under them.
    const kept: Record<StripKey, PageFace[]> = {};
    await Promise.all(
      Object.entries(extra).map(async ([key, loaded]) => {
        const group = groupOf(key, next);
        const { section } = parse(key);
        if (!group || !loaded.length) return;
        try {
          kept[key] = await refetch(group, section as Section, loaded.length);
        } catch (e) {
          // That strip folds back to its first faces; the page itself still loads.
          deps.reportError(e);
        }
      }),
    );
    // A strip the user expanded or folded since the reload began stays as they left it.
    for (const key of new Set([...Object.keys(kept), ...Object.keys(extra)])) {
      if ((generation[key] ?? 0) === (before[key] ?? 0)) continue;
      if (key in extra) kept[key] = extra[key];
      else delete kept[key];
    }
    // Together, after the re-fetch: the new base with the old extra would show a face twice.
    // People and Suggestions are by name, which a reload moves only for a renamed person.
    page = {
      ...next,
      unnamed: keepOrder(page?.unnamed, next.unnamed),
      ignoredGroups: keepOrder(page?.ignoredGroups, next.ignoredGroups),
    };
    extra = kept;
    settle(started);
  }

  const load = singleFlight(fetch);

  type Hidden = 'faces' | 'groups';
  /** The map is looked up by name at each step, never held: a change reassigns it so a
   *  component re-reads it, and a held reference would mutate the one just replaced. */
  const mapOf = (which: Hidden) => (which === 'faces' ? hiddenFaces : hiddenGroups);
  function publish() {
    hiddenFaces = new Map(hiddenFaces);
    hiddenGroups = new Map(hiddenGroups);
  }

  /** Runs `write` with `ids` hidden; puts them back if it fails. Reloads either way: a
   *  failure can mean the page is stale (a group a grouping run deleted). The reload is
   *  asked for after the write's end, which reads `tick` without taking one; the reload's own
   *  `++tick` is what numbers it past that end, so it counts as started after the write. */
  async function optimistic(which: Hidden, ids: number[], write: () => Promise<unknown>) {
    for (const id of ids) mapOf(which).set(id, null);
    publish();
    try {
      await write();
      // Not a new tick: a reload started from here on numbers itself past this one, and one
      // started earlier carries a number at or below it, which `settle` keeps.
      const at = tick;
      for (const id of ids) if (mapOf(which).has(id)) mapOf(which).set(id, at);
    } catch (e) {
      for (const id of ids) mapOf(which).delete(id);
      deps.reportError(e);
    }
    publish();
    await load().catch(deps.reportError);
  }

  function nameOf(id: number): string {
    return page?.people.find((p) => p.id === id)?.name ?? '';
  }

  return {
    get page() { return page; },
    get unnamed() { return (page?.unnamed ?? []).filter((g) => !hiddenGroups.has(g.id)); },
    get suggestions() { return page?.suggestions ?? []; },
    get people() { return page?.people ?? []; },
    get ignoredGroups() { return (page?.ignoredGroups ?? []).filter((g) => !hiddenGroups.has(g.id)); },
    load,
    faces,
    count,
    canShowMore: (key: StripKey) => key !== SINGLE && key !== IGNORED_FACES && faces(key).length < count(key),
    isExpanded: (key: StripKey) => (extra[key]?.length ?? 0) > 0,
    async showMore(key: StripKey) {
      const group = groupOf(key);
      if (!group) return;
      const { section } = parse(key);
      const offset = group.faces.length + (extra[key]?.length ?? 0);
      try {
        const more = await deps.more(group.id, FILTER[section as Section], offset, MORE);
        extra = { ...extra, [key]: [...(extra[key] ?? []), ...more] };
        bump(key);
      } catch (e) {
        deps.reportError(e);
      }
    },
    showFewer(key: StripKey) {
      const rest = { ...extra };
      delete rest[key];
      extra = rest;
      bump(key);
    },
    selected: (key: StripKey) => (selection?.strip === key ? selection.ids : []),
    isSelected: (key: StripKey, id: number) => selection?.strip === key && selection.ids.includes(id),
    toggle(key: StripKey, id: number) {
      if (selection?.strip !== key) selection = { strip: key, ids: [id] };
      else if (selection.ids.includes(id)) {
        const ids = selection.ids.filter((x) => x !== id);
        selection = ids.length ? { strip: key, ids } : null;
      } else selection = { strip: key, ids: [...selection.ids, id] };
    },
    clearSelection() { selection = null; },
    actionsFor: (key: StripKey) => ACTIONS[parse(key).section],
    async act(key: StripKey, action: FaceAction) {
      const ids = selection?.strip === key ? selection.ids : [];
      if (!ids.length) return;
      selection = null;
      const write =
        action === 'confirm' ? () => deps.confirm(ids)
        : action === 'reject' ? () => deps.reject(ids)
        : action === 'ignore' ? () => deps.ignoreFaces(ids, true)
        : () => deps.ignoreFaces(ids, false);
      await optimistic('faces', ids, write);
    },
    confirmAllLabel(person: number): string {
      const key = stripKey('suggestion', person);
      const n = faces(key).length;
      return n < count(key) ? `Confirm these ${n}` : 'Confirm all';
    },
    async confirmAll(person: number) {
      const ids = faces(stripKey('suggestion', person)).map((f) => f.id);
      if (ids.length) await optimistic('faces', ids, () => deps.confirm(ids));
    },
    choice: (typed: string, self?: number): NameChoice => nameChoice(typed, page?.people ?? [], self),
    async nameGroup(group: number, typed: string) {
      const name = typed.trim();
      if (!name) return;
      await optimistic('groups', [group], () => deps.name(group, name));
    },
    async nameSingles(typed: string) {
      const name = typed.trim();
      const chosen = faces(SINGLE).filter((f) => selection?.strip === SINGLE && selection.ids.includes(f.id));
      const groups = chosen.map((f) => f.personId).filter((g): g is number => g !== null);
      if (!name || !groups.length) return;
      selection = null;
      await optimistic('faces', chosen.map((f) => f.id), async () => {
        const person = await deps.name(groups[0], name);
        for (const g of groups.slice(1)) await deps.merge(g, person);
      });
    },
    async rename(person: number, typed: string) {
      const choice = nameChoice(typed, page?.people ?? [], person);
      if (choice.kind === 'empty' || choice.kind === 'same') return;
      try {
        await deps.rename(person, typed.trim());
      } catch (e) {
        deps.reportError(e);
      }
      await load().catch(deps.reportError);
    },
    async merge(from: number, into: number) {
      const ok = await deps.ask(
        `Merge “${nameOf(from)}” into “${nameOf(into)}”? Their faces become ${nameOf(into)}'s. This cannot be undone.`,
        'Merge people',
      );
      if (!ok) return;
      try {
        await deps.merge(from, into);
      } catch (e) {
        deps.reportError(e);
      }
      await load().catch(deps.reportError);
    },
    async remove(person: number) {
      const ok = await deps.ask(
        `Delete “${nameOf(person)}”? Their faces stay together as a group with no name.`,
        'Delete person',
      );
      if (!ok) return;
      try {
        await deps.remove(person);
      } catch (e) {
        deps.reportError(e);
      }
      await load().catch(deps.reportError);
    },
    async ignoreGroup(group: number, ignored: boolean) {
      await optimistic('groups', [group], () => deps.ignoreGroup(group, ignored));
    },
  };
}

export type PeoplePageModel = ReturnType<typeof createPeoplePage>;
