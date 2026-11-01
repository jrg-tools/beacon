<script lang="ts">
	/** A sparkline: one column per day; hover a column for its value. */
	interface Props {
		title: string;
		data: { label: string; value: number }[];
		height?: number;
	}
	let { title, data, height = 24 }: Props = $props();

	let width = $state(0);
	let hover = $state<number | null>(null);
	const peak = $derived(Math.max(0, ...data.map((d) => d.value)));
	const slot = $derived(width / Math.max(1, data.length));
	// At most 24px wide, with a 2px gap between neighbours.
	const bar = $derived(Math.max(1, Math.min(24, slot - 2)));

	// Rounded at the data end, square at the baseline.
	function column(i: number, value: number) {
		const h = (value / Math.max(1, peak)) * height;
		const x = i * slot + (slot - bar) / 2;
		const y = height - h;
		const r = Math.min(1, h, bar / 2);
		return `M${x},${height}V${y + r}Q${x},${y} ${x + r},${y}H${x + bar - r}Q${x + bar},${y} ${x + bar},${y + r}V${height}Z`;
	}
</script>

<figure class="relative">
	<!-- Pointer only: the svg's label is what screen readers get. -->
	<div
		role="presentation"
		bind:clientWidth={width}
		onpointermove={(e) =>
			(hover = Math.min(data.length - 1, Math.max(0, Math.floor(e.offsetX / slot))))}
		onpointerleave={() => (hover = null)}
	>
		<svg
			{width}
			{height}
			class="block overflow-visible"
			role="img"
			aria-label="{title}, peak {peak}"
		>
			<line
				x1="0"
				x2={width}
				y1={height - 0.5}
				y2={height - 0.5}
				class="stroke-zinc-200 dark:stroke-zinc-800"
			/>
			{#each data as d, i (d.label)}
				{#if d.value > 0}
					<path
						d={column(i, d.value)}
						class="fill-amber-600 transition-opacity"
						opacity={hover === null || hover === i ? 1 : 0.45}
					/>
				{/if}
			{/each}
		</svg>
	</div>
	{#if hover !== null && data[hover]}
		<div
			class="pointer-events-none absolute bottom-full z-10 mb-1 -translate-x-1/2 rounded-lg border border-zinc-200 bg-white px-2 py-1 text-xs whitespace-nowrap shadow-sm dark:border-zinc-700 dark:bg-zinc-900"
			style:left="{Math.min(Math.max((hover + 0.5) * slot, 40), width - 40)}px"
		>
			<span class="text-zinc-500">{data[hover].label}</span>
			<span class="font-semibold">{data[hover].value}</span>
		</div>
	{/if}
</figure>
