import { afterEach, describe, expect, it, vi } from 'vitest';
import type { FaceFilter, PageFace, PageGroup, PeoplePage } from './api';
import {
  CHANGE_GAP_MS,
  IGNORED_FACES,
  MORE,
  SINGLE,
  STALE_GROUP,
  STRIP,
  createPeoplePage,
  stripKey,
  type PeoplePageDeps,
} from './people-page.svelte';

function deferred<T = void>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const face = (id: number, personId: number | null = null, confirmed = false): PageFace => ({
  id,
  itemId: id,
  thumbKey: 'k' + id,
  confirmed,
  personId,
});
const group = (id: number, name: string | null, faces: PageFace[], faceCount = faces.length): PageGroup => ({
  id,
  name,
  faceCount,
  faces,
  offer: null,
});
const answer = (over: Partial<PeoplePage> = {}): PeoplePage => ({
  unnamed: [],
  singleFaces: [],
  singleCount: 0,
  suggestions: [],
  people: [],
  ignoredGroups: [],
  ignoredFaces: [],
  ...over,
  unnamedCount: over.unnamedCount ?? over.unnamed?.length ?? 0,
});

/** A page whose deps are all `vi.fn`s that resolve at once, unless a test replaces them. */
function build(first: PeoplePage) {
  const deps = {
    load: vi.fn(async (_strip: number) => first),
    more: vi.fn(async (_p: number, _w: FaceFilter, _o: number, _l: number): Promise<PageFace[]> => []),
    name: vi.fn(async (g: number, _n: string) => g),
    rename: vi.fn(async (p: number, _n: string) => p),
    confirm: vi.fn(async (_f: number[]) => {}),
    reject: vi.fn(async (_f: number[]) => {}),
    merge: vi.fn(async (_a: number, _b: number) => {}),
    ignoreGroup: vi.fn(async (_g: number, _i: boolean) => {}),
    ignoreFaces: vi.fn(async (_f: number[], _i: boolean) => {}),
    remove: vi.fn(async (_p: number) => {}),
    ask: vi.fn(async (_m: string, _t: string) => true),
    reportError: vi.fn(),
  } satisfies PeoplePageDeps;
  return { deps, model: createPeoplePage(deps) };
}

const unnamedPage = () => answer({ unnamed: [group(1, null, [face(1), face(2), face(3)], 5)] });
const A = stripKey('unnamed', 1);

describe('createPeoplePage', () => {
  it('loads the sections', async () => {
    const { model } = build(unnamedPage());
    await model.load();
    expect(model.page).not.toBeNull();
    expect(model.faces(A).map((f) => f.id)).toEqual([1, 2, 3]);
    expect(model.count(A)).toBe(5);
  });

  it('selects within one strip at a time', async () => {
    const { model } = build(unnamedPage());
    const b = stripKey('person', 2);
    model.toggle(A, 1);
    model.toggle(A, 2);
    expect(model.selected(A)).toEqual([1, 2]);
    model.toggle(b, 7);
    expect(model.selected(A)).toEqual([]);
    expect(model.selected(b)).toEqual([7]);
    model.toggle(b, 7);
    expect(model.selected(b)).toEqual([]);
  });

  it('offers the actions each section allows', () => {
    const { model } = build(answer());
    expect(model.actionsFor(stripKey('unnamed', 1))).toEqual(['reject', 'ignore']);
    expect(model.actionsFor(stripKey('suggestion', 1))).toEqual(['confirm', 'reject', 'ignore']);
    expect(model.actionsFor(stripKey('person', 1))).toEqual(['reject', 'ignore']);
    expect(model.actionsFor(SINGLE)).toEqual(['ignore']);
    expect(model.actionsFor(IGNORED_FACES)).toEqual(['unignore']);
    expect(model.actionsFor(stripKey('ignored', 1))).toEqual([]);
  });

  it('an action hides its faces at once and reloads after', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    const write = deferred();
    deps.reject.mockReturnValueOnce(write.promise);
    model.toggle(A, 1);
    model.toggle(A, 2);
    const done = model.act(A, 'reject');
    expect(model.faces(A).map((f) => f.id)).toEqual([3]);
    expect(model.count(A)).toBe(3);
    expect(model.selected(A)).toEqual([]);
    expect(deps.reject).toHaveBeenCalledWith([1, 2]);
    expect(deps.load).toHaveBeenCalledTimes(1);
    write.resolve();
    await done;
    expect(deps.load).toHaveBeenCalledTimes(2);
  });

  it('a reload started before an action ends does not bring its faces back', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    const write = deferred();
    deps.reject.mockReturnValueOnce(write.promise);
    model.toggle(A, 1);
    const done = model.act(A, 'reject');
    // A reload that began while the write was in flight reads the backend before it.
    const stale = deferred<PeoplePage>();
    deps.load.mockReturnValueOnce(stale.promise);
    const reload = model.load();
    write.resolve();
    await Promise.resolve();
    stale.resolve(unnamedPage());
    await reload;
    expect(model.faces(A).map((f) => f.id)).toEqual([2, 3]);
    // The action's own reload then runs after the write and may let the face go.
    await done;
  });

  it('a reload started after it shows what the backend says', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    model.toggle(A, 1);
    await model.act(A, 'reject');
    // Its own reload was answered with face 1 still present: the backend has the last word.
    expect(model.faces(A).map((f) => f.id)).toEqual([1, 2, 3]);
    expect(deps.load).toHaveBeenCalledTimes(2);
  });

  it('a failed action puts its faces back and reports', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    const err = new Error('no');
    const write = deferred();
    deps.reject.mockReturnValueOnce(write.promise);
    model.toggle(A, 1);
    const done = model.act(A, 'reject');
    expect(model.faces(A).map((f) => f.id)).toEqual([2, 3]);
    write.reject(err);
    await done;
    expect(model.faces(A).map((f) => f.id)).toEqual([1, 2, 3]);
    expect(deps.reportError).toHaveBeenCalledWith(err);
  });

  it('confirm all confirms the faces on screen', async () => {
    const twelve = Array.from({ length: 12 }, (_, i) => face(i + 1));
    const { deps, model } = build(answer({ suggestions: [group(4, 'Ben', twelve, 30)] }));
    await model.load();
    expect(model.confirmAllLabel(4)).toBe('Confirm these 12');
    await model.confirmAll(4);
    expect(deps.confirm).toHaveBeenCalledWith(twelve.map((f) => f.id));
    const all = build(answer({ suggestions: [group(4, 'Ben', twelve)] }));
    await all.model.load();
    expect(all.model.confirmAllLabel(4)).toBe('Confirm all');
  });

  it('show more pages through the group with the section filter', async () => {
    const strip = (n: number) => Array.from({ length: STRIP }, (_, i) => face(n * 100 + i));
    const { deps, model } = build(
      answer({
        people: [group(1, 'Anna', strip(1), 40)],
        suggestions: [group(2, 'Ben', strip(2), 40)],
        unnamed: [group(3, null, strip(3), 40)],
        ignoredGroups: [group(4, null, strip(4), 40)],
      }),
    );
    await model.load();
    deps.more.mockResolvedValue([face(900), face(901)]);
    await model.showMore(stripKey('person', 1));
    expect(deps.more).toHaveBeenLastCalledWith(1, 'confirmed', STRIP, MORE);
    expect(model.faces(stripKey('person', 1)).map((f) => f.id)).toContain(901);
    expect(model.faces(stripKey('person', 1))).toHaveLength(STRIP + 2);
    await model.showMore(stripKey('suggestion', 2));
    expect(deps.more).toHaveBeenLastCalledWith(2, 'unconfirmed', STRIP, MORE);
    await model.showMore(stripKey('unnamed', 3));
    expect(deps.more).toHaveBeenLastCalledWith(3, 'all', STRIP, MORE);
    await model.showMore(stripKey('ignored', 4));
    expect(deps.more).toHaveBeenLastCalledWith(4, 'all', STRIP, MORE);
    model.showFewer(stripKey('person', 1));
    expect(model.faces(stripKey('person', 1))).toHaveLength(STRIP);
  });

  it('a reload keeps what show more loaded', async () => {
    const first = Array.from({ length: STRIP }, (_, i) => face(i + 1, null, true));
    const page = answer({ people: [group(1, 'Anna', first, 40)] });
    const { deps, model } = build(page);
    await model.load();
    const key = stripKey('person', 1);
    deps.more.mockResolvedValue(Array.from({ length: 28 }, (_, i) => face(100 + i, null, true)));
    await model.showMore(key);
    expect(model.faces(key)).toHaveLength(40);
    await model.load();
    expect(deps.more).toHaveBeenLastCalledWith(1, 'confirmed', STRIP, 28);
    expect(model.faces(key)).toHaveLength(40);
  });

  /** The backend's `person_faces`: faces `offset..` of a group of `total`, at most `MORE` a
   *  call however many are asked for, as `MAX_FACE_PAGE` clamps them. */
  const clamped =
    (total: number) =>
    async (_p: number, _w: FaceFilter, offset: number, limit: number): Promise<PageFace[]> =>
      Array.from({ length: Math.max(0, Math.min(limit, MORE, total - offset)) }, (_, i) => face(offset + i));

  it('a reload re-fetches every page show more loaded, a page at a time', async () => {
    const first = Array.from({ length: STRIP }, (_, i) => face(i));
    const { deps, model } = build(answer({ unnamed: [group(1, null, first, 1000)] }));
    deps.more.mockImplementation(clamped(1000));
    await model.load();
    for (let i = 0; i < 3; i++) await model.showMore(A);
    expect(model.faces(A)).toHaveLength(STRIP + 3 * MORE);
    await model.load();
    expect(model.faces(A).map((f) => f.id)).toEqual(Array.from({ length: STRIP + 3 * MORE }, (_, i) => i));
  });

  it('a re-fetch stops at a short page', async () => {
    const first = Array.from({ length: STRIP }, (_, i) => face(i));
    const { deps, model } = build(answer({ unnamed: [group(1, null, first, 1000)] }));
    deps.more.mockImplementation(clamped(1000));
    await model.load();
    await model.showMore(A);
    await model.showMore(A);
    // Faces left since: the group now ends 250 faces in.
    deps.more.mockClear();
    deps.more.mockImplementation(clamped(250));
    await model.load();
    expect(model.faces(A)).toHaveLength(250);
    expect(deps.more).toHaveBeenCalledTimes(2);
  });

  it('show fewer while a reload re-fetches the strip stays folded', async () => {
    const first = Array.from({ length: STRIP }, (_, i) => face(i));
    const { deps, model } = build(answer({ unnamed: [group(1, null, first, 40)] }));
    deps.more.mockImplementation(clamped(40));
    await model.load();
    await model.showMore(A);
    const refetch = deferred<PageFace[]>();
    deps.more.mockReturnValueOnce(refetch.promise);
    const reload = model.load();
    await vi.waitFor(() => expect(deps.more).toHaveBeenCalledTimes(2));
    model.showFewer(A);
    refetch.resolve(Array.from({ length: 28 }, (_, i) => face(STRIP + i)));
    await reload;
    expect(model.isExpanded(A)).toBe(false);
    expect(model.faces(A)).toHaveLength(STRIP);
  });

  it('show more while a reload re-fetches the strip is kept', async () => {
    const first = Array.from({ length: STRIP }, (_, i) => face(i));
    const { deps, model } = build(answer({ unnamed: [group(1, null, first, 1000)] }));
    deps.more.mockImplementation(clamped(1000));
    await model.load();
    await model.showMore(A);
    const refetch = deferred<PageFace[]>();
    deps.more.mockReturnValueOnce(refetch.promise);
    const reload = model.load();
    await vi.waitFor(() => expect(deps.more).toHaveBeenCalledTimes(2));
    await model.showMore(A);
    expect(model.faces(A)).toHaveLength(STRIP + 2 * MORE);
    refetch.resolve(Array.from({ length: MORE }, (_, i) => face(STRIP + i)));
    await reload;
    expect(model.faces(A)).toHaveLength(STRIP + 2 * MORE);
  });

  it('a face read by both the strip and show more is listed once', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    deps.more.mockResolvedValueOnce([face(3), face(4)]);
    await model.showMore(A);
    expect(model.faces(A).map((f) => f.id)).toEqual([1, 2, 3, 4]);
    expect(model.count(A)).toBe(5);
  });

  it('a reload keeps the order of the groups already shown', async () => {
    const two = [face(1), face(2)];
    const { deps, model } = build(
      answer({
        unnamed: [group(1, null, two, 9), group(2, null, two, 5), group(3, null, two, 3)],
        ignoredGroups: [group(7, null, two, 4), group(8, null, two, 2)],
      }),
    );
    await model.load();
    // Group 2 has grown past group 1, group 3 has gone, group 4 is new and the largest;
    // the ignored groups' counts have swapped and group 9 is new.
    deps.load.mockResolvedValueOnce(
      answer({
        unnamed: [group(4, null, two, 20), group(2, null, two, 12), group(1, null, two, 9)],
        ignoredGroups: [group(9, null, two, 6), group(8, null, two, 5), group(7, null, two, 4)],
      }),
    );
    await model.load();
    expect(model.unnamed.map((g) => g.id)).toEqual([1, 2, 4]);
    expect(model.unnamed.map((g) => g.faceCount)).toEqual([9, 12, 20]);
    expect(model.ignoredGroups.map((g) => g.id)).toEqual([7, 8, 9]);
  });

  it('new groups keep the backend order among themselves', async () => {
    const two = [face(1), face(2)];
    const { deps, model } = build(answer({ unnamed: [group(1, null, two, 3)] }));
    await model.load();
    deps.load.mockResolvedValueOnce(
      answer({ unnamed: [group(6, null, two, 9), group(5, null, two, 7), group(1, null, two, 3)] }),
    );
    await model.load();
    expect(model.unnamed.map((g) => g.id)).toEqual([1, 6, 5]);
  });

  it('counts every unnamed group, less those an action is taking away', async () => {
    const two = [face(1), face(2)];
    const { deps, model } = build(
      answer({ unnamed: [group(1, null, two), group(2, null, [face(3), face(4)])], unnamedCount: 250 }),
    );
    await model.load();
    expect(model.unnamedCount).toBe(250);
    const write = deferred<number>();
    deps.name.mockReturnValueOnce(write.promise);
    const done = model.nameGroup(1, 'Ben');
    expect(model.unnamedCount).toBe(249);
    expect(model.unnamed).toHaveLength(1);
    write.resolve(1);
    await done;
  });

  it('naming a group hides it from Unnamed and reloads', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    const write = deferred<number>();
    deps.name.mockReturnValueOnce(write.promise);
    const done = model.nameGroup(1, ' Ben ');
    expect(deps.name).toHaveBeenCalledWith(1, 'Ben');
    expect(model.unnamed).toHaveLength(0);
    write.resolve(1);
    await done;
    expect(deps.load).toHaveBeenCalledTimes(2);
    deps.name.mockClear();
    await model.nameGroup(1, '  ');
    expect(deps.name).not.toHaveBeenCalled();
  });

  it('a refused name reloads the page and says the group changed', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    deps.name.mockRejectedValueOnce({ kind: 'notAPerson', message: 'that is not a named person' });
    await model.nameGroup(1, 'Ben');
    expect(model.unnamed).toHaveLength(1);
    expect(deps.reportError).toHaveBeenCalledWith(STALE_GROUP);
    expect(deps.load).toHaveBeenCalledTimes(2);
  });

  it('a rename or merge of someone gone says the group changed; other errors pass through', async () => {
    const { deps, model } = build(answer({ people: [group(3, 'Anna', []), group(5, 'Ben', [])] }));
    await model.load();
    const gone = { kind: 'notAPerson', message: 'that is not a named person' };
    deps.rename.mockRejectedValueOnce(gone);
    await model.rename(3, 'Anne');
    deps.merge.mockRejectedValueOnce(gone);
    await model.merge(3, 5);
    const other = { kind: 'internal', message: 'disk full' };
    deps.merge.mockRejectedValueOnce(other);
    await model.merge(3, 5);
    expect(deps.reportError.mock.calls).toEqual([[STALE_GROUP], [STALE_GROUP], [other]]);
  });

  it('naming single faces names the first group and merges the rest into it', async () => {
    const { deps, model } = build(
      answer({ singleFaces: [face(1, 21), face(2, 22), face(3, 23)], singleCount: 3 }),
    );
    await model.load();
    deps.name.mockResolvedValueOnce(30);
    for (const id of [1, 2, 3]) model.toggle(SINGLE, id);
    await model.nameSingles('Ben');
    expect(deps.name).toHaveBeenCalledWith(21, 'Ben');
    expect(deps.merge.mock.calls).toEqual([
      [22, 30],
      [23, 30],
    ]);
    expect(deps.name.mock.invocationCallOrder[0]).toBeLessThan(deps.merge.mock.invocationCallOrder[0]);
  });

  it('merge and delete ask first, and do nothing when declined', async () => {
    const { deps, model } = build(answer({ people: [group(3, 'Anna', []), group(5, 'Ben', [])] }));
    await model.load();
    deps.ask.mockResolvedValue(false);
    await model.merge(3, 5);
    await model.remove(3);
    expect(deps.merge).not.toHaveBeenCalled();
    expect(deps.remove).not.toHaveBeenCalled();
    expect(deps.ask.mock.calls[0][0]).toContain('Anna');
    expect(deps.ask.mock.calls[0][0]).toContain('Ben');
    expect(deps.ask.mock.calls[1][0]).toContain('Anna');
    deps.ask.mockResolvedValue(true);
    deps.load.mockClear();
    await model.merge(3, 5);
    expect(deps.merge).toHaveBeenCalledWith(3, 5);
    await model.remove(3);
    expect(deps.remove).toHaveBeenCalledWith(3);
    expect(deps.load).toHaveBeenCalledTimes(2);
  });

  it('renaming to the same name does nothing', async () => {
    const { deps, model } = build(answer({ people: [group(3, 'Anna', [])] }));
    await model.load();
    await model.rename(3, ' Anna ');
    expect(deps.rename).not.toHaveBeenCalled();
    await model.rename(3, 'ANNA');
    expect(deps.rename).toHaveBeenCalledWith(3, 'ANNA');
    await model.rename(3, ' Anne ');
    expect(deps.rename).toHaveBeenLastCalledWith(3, 'Anne');
  });

  it('a failing show-more re-fetch still loads the page and drops only that strip', async () => {
    const strip = (n: number) => Array.from({ length: STRIP }, (_, i) => face(n * 100 + i, null, true));
    const page = answer({ people: [group(1, 'Anna', strip(1), 40), group(2, 'Ben', strip(2), 40)] });
    const { deps, model } = build(page);
    await model.load();
    deps.more.mockResolvedValue([face(900), face(901)]);
    await model.showMore(stripKey('person', 1));
    await model.showMore(stripKey('person', 2));
    const err = new Error('more');
    deps.more.mockImplementation(async (p) => {
      if (p === 1) throw err;
      return [face(950), face(951)];
    });
    deps.load.mockResolvedValueOnce(answer({ people: [group(1, 'Anna', strip(1), 41), group(2, 'Ben', strip(2), 40)] }));
    await model.load();
    expect(deps.reportError).toHaveBeenCalledWith(err);
    expect(model.count(stripKey('person', 1))).toBe(41);
    expect(model.faces(stripKey('person', 1))).toHaveLength(STRIP);
    expect(model.faces(stripKey('person', 2))).toHaveLength(STRIP + 2);
  });

  it('a selection loses faces a reload no longer shows', async () => {
    const { deps, model } = build(unnamedPage());
    await model.load();
    model.toggle(A, 1);
    model.toggle(A, 2);
    deps.load.mockResolvedValueOnce(answer({ unnamed: [group(1, null, [face(1), face(3)], 4)] }));
    await model.load();
    expect(model.selected(A)).toEqual([1]);
  });

  describe('library changes', () => {
    afterEach(() => {
      vi.useRealTimers();
    });

    it('reload at most once a gap, the last of a burst at its end', async () => {
      vi.useFakeTimers();
      const { deps, model } = build(unnamedPage());
      // At once, not on a timer, however short: the page opens on this call.
      model.changed();
      expect(deps.load).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(0);
      for (let i = 0; i < 5; i++) {
        await vi.advanceTimersByTimeAsync(100);
        model.changed();
      }
      expect(deps.load).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(CHANGE_GAP_MS - 500 - 1);
      expect(deps.load).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(deps.load).toHaveBeenCalledTimes(2);
      // A change long after the last reload is not held back.
      await vi.advanceTimersByTimeAsync(5 * CHANGE_GAP_MS);
      model.changed();
      await vi.advanceTimersByTimeAsync(0);
      expect(deps.load).toHaveBeenCalledTimes(3);
    });

    it("the page's own actions reload at once, and the change they announce waits", async () => {
      vi.useFakeTimers();
      const { deps, model } = build(unnamedPage());
      model.changed();
      await vi.advanceTimersByTimeAsync(5 * CHANGE_GAP_MS);
      model.toggle(A, 1);
      await model.act(A, 'reject');
      expect(deps.load).toHaveBeenCalledTimes(2);
      model.changed();
      await vi.advanceTimersByTimeAsync(0);
      expect(deps.load).toHaveBeenCalledTimes(2);
      await vi.advanceTimersByTimeAsync(CHANGE_GAP_MS);
      expect(deps.load).toHaveBeenCalledTimes(3);
    });

    it('a reload put off is dropped when the page goes', async () => {
      vi.useFakeTimers();
      const { deps, model } = build(unnamedPage());
      model.changed();
      await vi.advanceTimersByTimeAsync(0);
      model.changed();
      model.dispose();
      await vi.advanceTimersByTimeAsync(5 * CHANGE_GAP_MS);
      expect(deps.load).toHaveBeenCalledTimes(1);
    });
  });
});
