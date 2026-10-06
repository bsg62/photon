<script lang="ts" generics="T extends string | number">
  import { createSelect } from '../lib/select.svelte';
  import Icon from './Icon.svelte';

  /** photon's dropdown, drawn entirely by photon - closed and open - so it looks the same on
   *  every platform and like the rest of the UI: the closed control is a field like the
   *  Settings buttons, the open list is the right-click menu's surface. The keys and the
   *  active option are `createSelect`'s; this is the wiring. */
  let {
    options,
    value,
    onchange,
    label,
    placement = 'below',
    disabled = false,
  }: {
    options: { value: T; label: string }[];
    value: T;
    onchange: (value: T) => void;
    /** The accessible name: a select shows only its value, never what it is for. */
    label: string;
    /** Which way the list opens. `above` for a control near the bottom of the window. */
    placement?: 'below' | 'above';
    /** Shown, focusable and named, but not openable: for a choice that does not apply right
     *  now and will again, where hiding it would move its neighbours. */
    disabled?: boolean;
  } = $props();

  const id = $props.id();
  const held = $derived(options.findIndex((o) => o.value === value));
  const select = createSelect({
    count: () => options.length,
    selected: () => held,
    label: (i) => options[i].label,
    choose: (i) => onchange(options[i].value),
    disabled: () => disabled,
  });

  let root = $state<HTMLDivElement | undefined>();

  function onkeydown(e: KeyboardEvent) {
    // Stopped here as well as prevented: the viewer's keys live on the window, and a key the
    // list answered - Enter choosing, Escape closing - must not also apply or cancel a crop.
    if (select.key(e)) {
      e.preventDefault();
      e.stopPropagation();
    }
  }
</script>

<!-- A press anywhere else closes the list, as leaving a menu does. -->
<svelte:window onpointerdown={(e) => select.open && !root?.contains(e.target as Node) && select.close()} />

<div class="select" bind:this={root}>
  <!-- A focusable div, not a <button>: a button turns Space into a click on keyup, which
       would reopen the list the keydown just chose from. Focus stays here while the list is
       open; the active option is announced through aria-activedescendant (the APG's
       select-only combobox). -->
  <div
    class="field"
    class:open={select.open}
    class:disabled
    role="combobox"
    tabindex="0"
    aria-label={label}
    aria-haspopup="listbox"
    aria-expanded={select.open}
    aria-disabled={disabled}
    aria-controls="{id}-list"
    aria-activedescendant={select.open ? `${id}-${select.active}` : undefined}
    onclick={() => select.toggle()}
    {onkeydown}
    onblur={() => select.close()}
  >
    <span class="value">{options[held]?.label ?? ''}</span>
    <span class="chevron"><Icon name="chevron-down" size={14} /></span>
  </div>
  {#if select.open}
    <!-- The list never takes focus: a mousedown on it would move focus off the field, whose
         blur closes the list before the click could choose. -->
    <ul id="{id}-list" class="list" class:above={placement === 'above'} role="listbox" aria-label={label} onmousedown={(e) => e.preventDefault()}>
      {#each options as option, i (option.value)}
        <!-- Keys are the field's (aria-activedescendant); the pointer is all an option
             answers itself. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="{id}-{i}"
          role="option"
          aria-selected={i === held}
          class:active={i === select.active}
          onpointermove={() => select.hover(i)}
          onclick={() => select.commit(i)}
        >
          <span class="check">{#if i === held}<Icon name="check" size={14} />{/if}</span>
          {option.label}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .select { position: relative; display: inline-flex; flex: 0 0 auto; }
  /* The field: the Settings buttons' look (--field at rest, --field-hover under the pointer)
     at the viewer tool buttons' 30px, which is also the top bar's gear. */
  .field {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    height: 30px;
    padding: 0 var(--s-2) 0 var(--s-3);
    border-radius: var(--r-3);
    background: var(--field);
    color: var(--text);
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
    transition: background-color 120ms ease-out;
  }
  .field:hover, .field.open { background: var(--field-hover); }
  /* Spelled with the hover state so a disabled field does not light up under the pointer. */
  .field.disabled, .field.disabled:hover { background: var(--field); color: var(--text-dim); cursor: default; }
  .chevron { display: inline-flex; color: var(--text-dim); }
  /* The right-click menu's surface (Grid.svelte's .menu): raised, a hairline, the menu shadow. */
  .list {
    position: absolute;
    top: calc(100% + var(--s-1));
    left: 0;
    z-index: 40;
    min-width: 100%;
    margin: 0;
    padding: var(--s-1);
    list-style: none;
    background: var(--raised);
    border-radius: var(--r-3);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .list.above { top: auto; bottom: calc(100% + var(--s-1)); }
  li {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: 6px 10px 6px var(--s-2);
    border-radius: var(--r-2);
    color: var(--text);
    white-space: nowrap;
    cursor: pointer;
  }
  li.active { background: var(--hover); }
  /* Always the icon's width, so every label starts in one column whether it is held or not. */
  .check { display: inline-flex; width: 14px; color: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .field { transition: none; } }
</style>
