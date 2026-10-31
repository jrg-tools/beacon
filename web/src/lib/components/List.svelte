<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';

	/** A boxed list with dividers, the look of jrg-life's admin lists (its Sortable, without dragging). */
	let { items, key, row }: { items: T[]; key: (item: T) => string | number; row: Snippet<[T]> } =
		$props();
</script>

<ul
	class="divide-y divide-zinc-200 overflow-hidden rounded-2xl border border-zinc-200 bg-white sm:rounded-3xl dark:divide-zinc-800 dark:border-zinc-800 dark:bg-zinc-900/60"
>
	{#each items as item (key(item))}
		<li>{@render row(item)}</li>
	{/each}
</ul>
