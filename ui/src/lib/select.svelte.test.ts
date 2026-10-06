import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createSelect, TYPEAHEAD_RESET_MS } from './select.svelte';

const LABELS = ['Date taken', 'Date modified', 'Name', 'Size'];

function make(selected = 0, labels = LABELS, disabled: () => boolean = () => false) {
  const chosen: number[] = [];
  let held = selected;
  const select = createSelect({
    count: () => labels.length,
    selected: () => held,
    label: (i) => labels[i],
    choose: (i) => {
      chosen.push(i);
      held = i;
    },
    disabled,
  });
  const press = (key: string, mods: { altKey?: boolean; ctrlKey?: boolean } = {}) => select.key({ key, ...mods });
  return { select, chosen, press };
}

describe('createSelect', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('does not open while disabled, from a click or from a key, and opens once it is not', () => {
    let off = true;
    const { select, press } = make(2, LABELS, () => off);
    select.toggle();
    expect(select.open).toBe(false);
    for (const key of ['ArrowDown', 'ArrowUp', 'Enter', ' ', 'Home', 'End', 'd']) {
      expect(press(key)).toBe(false);
      expect(select.open).toBe(false);
    }
    expect(select.active).toBe(0);
    off = false;
    select.toggle();
    expect(select.open).toBe(true);
    expect(select.active).toBe(2);
  });

  it('opens on the held option from the keys a closed select answers', () => {
    for (const key of ['ArrowDown', 'ArrowUp', 'Enter', ' ']) {
      const { select, press } = make(2);
      expect(press(key)).toBe(true);
      expect(select.open).toBe(true);
      expect(select.active).toBe(2);
    }
    const home = make(2);
    home.press('Home');
    expect(home.select.active).toBe(0);
    const end = make(0);
    end.press('End');
    expect(end.select.active).toBe(3);
  });

  it('leaves a closed select alone for keys it has no use for, Escape above all', () => {
    // Escape and Tab fall through to whatever else answers them: the viewer's Escape
    // cancels the crop, Tab moves focus.
    for (const key of ['Escape', 'Tab', 'F11', 'ArrowLeft']) {
      const { select, press } = make();
      expect(press(key)).toBe(false);
      expect(select.open).toBe(false);
    }
    // A shortcut is not type-ahead: Ctrl+A belongs to the app.
    const { select, press } = make();
    expect(press('a', { ctrlKey: true })).toBe(false);
    expect(select.open).toBe(false);
  });

  it('moves within the list without wrapping, and chooses with Enter', () => {
    const { select, chosen, press } = make(0);
    press('ArrowDown');
    press('ArrowDown');
    press('ArrowDown');
    press('ArrowDown');
    press('ArrowDown');
    expect(select.active).toBe(3);
    press('ArrowUp');
    expect(select.active).toBe(2);
    press('Home');
    expect(select.active).toBe(0);
    press('End');
    press('PageUp');
    expect(select.active).toBe(0);
    press('PageDown');
    expect(select.active).toBe(3);
    press('ArrowUp');
    expect(press('Enter')).toBe(true);
    expect(select.open).toBe(false);
    expect(chosen).toEqual([2]);
  });

  it('closes on Escape without choosing, and on Tab choosing but letting focus move', () => {
    const escaped = make(0);
    escaped.press('ArrowDown');
    escaped.press('ArrowDown');
    expect(escaped.press('Escape')).toBe(true);
    expect(escaped.select.open).toBe(false);
    expect(escaped.chosen).toEqual([]);

    const tabbed = make(0);
    tabbed.press('ArrowDown');
    tabbed.press('ArrowDown');
    expect(tabbed.press('Tab')).toBe(false);
    expect(tabbed.select.open).toBe(false);
    expect(tabbed.chosen).toEqual([1]);
  });

  it('chooses with Space, and with Alt+ArrowUp', () => {
    const space = make(0);
    space.press('ArrowDown');
    space.press('ArrowDown');
    space.press(' ');
    expect(space.chosen).toEqual([1]);
    const alt = make(0);
    alt.press('ArrowDown');
    alt.press('End');
    alt.press('ArrowUp', { altKey: true });
    expect(alt.chosen).toEqual([3]);
  });

  it('does not report the held option as a change', () => {
    const { chosen, press } = make(2);
    press('Enter');
    press('Enter');
    expect(chosen).toEqual([]);
  });

  it('types ahead: a word from the active option, a repeated letter cycling', () => {
    const { select, press } = make(3);
    press('ArrowDown'); // open on Size
    press('d');
    expect(select.active).toBe(0);
    press('a');
    press('t');
    press('e');
    press(' ');
    press('m');
    expect(select.active).toBe(1);
    // Space mid-word was part of the word, not a choice.
    expect(select.open).toBe(true);
    vi.advanceTimersByTime(TYPEAHEAD_RESET_MS);
    press('d');
    expect(select.active).toBe(0);
    press('d');
    expect(select.active).toBe(1);
    press('d');
    expect(select.active).toBe(0);
  });

  it('starts a new word after a pause', () => {
    const { select, press } = make(0);
    press('ArrowDown');
    press('n');
    expect(select.active).toBe(2);
    vi.advanceTimersByTime(TYPEAHEAD_RESET_MS);
    press('s');
    expect(select.active).toBe(3);
  });

  it('opens and types ahead from a closed select', () => {
    const { select, press } = make(0);
    expect(press('s')).toBe(true);
    expect(select.open).toBe(true);
    expect(select.active).toBe(3);
  });

  it('follows the pointer and chooses the option clicked', () => {
    const { select, chosen } = make(0);
    select.toggle();
    select.hover(2);
    expect(select.active).toBe(2);
    select.commit(3);
    expect(select.open).toBe(false);
    expect(chosen).toEqual([3]);
    select.toggle();
    select.toggle();
    expect(select.open).toBe(false);
  });

  it('holds nothing to open on when there are no options', () => {
    const { select, press } = make(0, []);
    expect(press('ArrowDown')).toBe(false);
    expect(select.open).toBe(false);
  });
});
