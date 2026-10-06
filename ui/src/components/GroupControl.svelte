<script lang="ts">
  import { GROUPINGS, groupingApplies } from '../lib/grouping';
  import { library } from '../lib/library.svelte';
  import Select from './Select.svelte';

  const sort = $derived(library.sort);
  const applies = $derived(groupingApplies(sort));
</script>

<!-- Holds no grouping of its own, like the sort control beside it: the grouping is a field
     of `library.sort`, so a change here builds on a sort change still in flight and a
     refused one puts both controls back.

     Disabled rather than hidden under a sort that ignores it: hidden, the top bar's controls
     would shift whenever the sort changed, and the choice it still holds - the one that
     comes back with Date taken - would be out of sight. -->
<div class="group" title={applies ? undefined : 'Grouping applies when sorted by date taken'}>
  <Select label="Group by" options={GROUPINGS} value={sort.group} disabled={!applies} onchange={(group) => library.setSort({ ...sort, group })} />
</div>

<style>
  /* `flex: 0 0 auto` for the same reason as the controls beside it: the search bar is the
     top bar's one child meant to give way. */
  .group { display: inline-flex; flex: 0 0 auto; }
</style>
