<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';

	/**
	 * A boxed list with dividers, the look of jrg-life's admin lists (its Sortable, without dragging).
	 * With `pageSize`, it shows that many at a time with a pager below.
	 */
	let {
		items,
		key,
		row,
		pageSize = Infinity
	}: {
		items: T[];
		key: (item: T) => string | number;
		row: Snippet<[T]>;
		pageSize?: number;
	} = $props();

	// shortcut: pages in the browser over the full list; move it to the API if a list gets thousands long.
	let page = $state(0);
	const pages = $derived(Math.max(1, Math.ceil(items.length / pageSize)));
	// Clamped, so a refresh that shortens the list never leaves an empty page.
	const current = $derived(Math.min(page, pages - 1));
	const shown = $derived(
		pageSize === Infinity ? items : items.slice(current * pageSize, (current + 1) * pageSize)
	);
</script>

<ul
	class="divide-y divide-zinc-200 overflow-hidden rounded-lg border border-zinc-200 bg-white dark:divide-zinc-800 dark:border-zinc-800 dark:bg-zinc-900/60"
>
	{#each shown as item (key(item))}
		<li>{@render row(item)}</li>
	{/each}
</ul>
{#if pages > 1}
	<nav class="mt-3 flex items-center justify-between text-xs text-zinc-500" aria-label="Pages">
		<button class="action" disabled={current === 0} onclick={() => (page = current - 1)}
			>← Previous</button
		>
		<span class="tabular-nums">{current + 1} / {pages}</span>
		<button class="action" disabled={current === pages - 1} onclick={() => (page = current + 1)}
			>Next →</button
		>
	</nav>
{/if}
