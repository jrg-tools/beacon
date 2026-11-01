<script lang="ts">
	import { size } from '#lib/format.ts';
	import type { ProjectStats } from '#lib/types.ts';

	let { stats }: { stats: ProjectStats } = $props();

	const sum = (key: 'checks' | 'downloads' | 'bytes') => stats.days.reduce((n, d) => n + d[key], 0);
	const pct = (n: number) => (stats.devices ? `${Math.round((n / stats.devices) * 100)}%` : '—');
	const versions = $derived(Object.entries(stats.versions).sort((a, b) => b[1] - a[1]));

	const tiles = $derived([
		{ label: 'Devices', value: stats.devices, note: `${stats.active_24h} active in 24 h` },
		{
			label: 'Up to date',
			value: pct(stats.up_to_date),
			note: stats.latest ? `${stats.up_to_date} on ${stats.latest} or newer` : 'no stable release'
		},
		{
			label: 'Active 7 days',
			value: stats.active_7d,
			note: `${stats.devices - stats.active_7d} not seen`
		},
		{ label: 'Updates installed', value: stats.updates, note: 'last 30 days' },
		{
			label: 'Update checks',
			value: sum('checks'),
			note: `last 30 days · ${stats.checks} all time`
		},
		{
			label: 'Downloads',
			value: sum('downloads'),
			note: `last 30 days · ${stats.downloads} all time`
		},
		{ label: 'Data served', value: size(sum('bytes')), note: 'last 30 days' },
		{ label: 'On beta', value: stats.channels.beta ?? 0, note: 'devices, last 30 days' }
	]);
</script>

<dl class="grid grid-cols-2 gap-3 sm:grid-cols-4">
	{#each tiles as tile (tile.label)}
		<div
			class="rounded-lg border border-zinc-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-900"
		>
			<dt class="text-xs text-zinc-500">{tile.label}</dt>
			<dd class="text-2xl font-semibold">{tile.value}</dd>
			<dd class="truncate text-[11px] text-zinc-400">{tile.note}</dd>
		</div>
	{/each}
</dl>

{#if versions.length}
	<div class="mt-6">
		<h3 class="mb-2 text-xs font-semibold text-zinc-500">Versions in the field</h3>
		<ul class="space-y-1.5">
			{#each versions as [version, count] (version)}
				<li class="grid grid-cols-[6rem_minmax(0,1fr)_3rem] items-center gap-3 text-xs">
					<span class="truncate font-medium">
						{version}{#if version === stats.latest}<span class="text-zinc-400">&nbsp;· latest</span
							>{/if}
					</span>
					<span class="h-3">
						<span
							class="block h-full rounded-r-[4px] bg-amber-600"
							style:width="{(count / stats.devices) * 100}%"
						></span>
					</span>
					<span class="text-right text-zinc-500">{count}</span>
				</li>
			{/each}
		</ul>
	</div>
{/if}
