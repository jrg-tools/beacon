<script lang="ts">
	import { size } from '#lib/format.ts';
	import type { ProjectStats } from '#lib/types.ts';

	let { stats }: { stats: ProjectStats } = $props();

	const sum = (key: 'checks' | 'downloads' | 'bytes') => stats.days.reduce((n, d) => n + d[key], 0);
	const pct = (n: number) => (stats.devices ? `${Math.round((n / stats.devices) * 100)}%` : '—');
	// Green for the latest, then 3 hues from the dataviz skill's palette, light and dark steps.
	// The devices list's "up to date" dot (bg-emerald-500), so the latest reads the same everywhere.
	const GREEN = ['var(--color-emerald-500)', 'var(--color-emerald-500)'];
	const PALETTE = [
		['#2a78d6', '#3987e5'],
		['#eda100', '#c98500'],
		['#e87ba4', '#d55181']
	];
	const GRAY = ['#a1a1aa', '#52525b']; // "unknown" and "other": not a version
	// As GitHub's language bar: biggest share first; the latest and 3 more, the rest fold into "other".
	// Colors follow the version (newest gets the first), so a count changing never repaints one.
	const versions = $derived.by(() => {
		const sorted = Object.entries(stats.versions).sort((a, b) => b[1] - a[1]);
		const others = sorted.filter(([v]) => v !== 'unknown' && v !== stats.latest);
		const top = others.slice(0, PALETTE.length);
		const byName = top
			.map(([v]) => v)
			.sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
		const rest = others.slice(PALETTE.length).reduce((n, [, c]) => n + c, 0);
		const latest = stats.latest ? (stats.versions[stats.latest] ?? 0) : 0;
		const unknown = stats.versions.unknown ?? 0;
		return [
			...top.map(([version, count]) => ({
				version,
				count,
				color: PALETTE[byName.indexOf(version)]
			})),
			...(latest ? [{ version: stats.latest!, count: latest, color: GREEN }] : []),
			...(rest ? [{ version: 'other', count: rest, color: GRAY }] : [])
		]
			.sort((a, b) => b.count - a.count)
			.concat(unknown ? [{ version: 'unknown', count: unknown, color: GRAY }] : []);
	});
	let counts = $state(false);
	const share = (n: number) => `${((n / stats.devices) * 100).toFixed(1)}%`;

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
		{
			label: 'Updates installed',
			value: stats.updates,
			note: `last 30 days · ${stats.updates_pending} pending · ${stats.updates_failed} failed`
		},
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
			<dd class="text-[11px] text-zinc-400 sm:truncate">{tile.note}</dd>
		</div>
	{/each}
</dl>

{#if versions.length}
	<div class="mt-6">
		<h3 class="mb-2 text-xs font-semibold text-zinc-500">Versions in the field</h3>
		<!-- Click for device counts instead of shares, as GitHub's language list. -->
		<button
			class="block w-full cursor-pointer space-y-2.5 text-left"
			onclick={() => (counts = !counts)}
			aria-pressed={counts}
			title={counts ? 'Show shares' : 'Show device counts'}
		>
			<span class="flex h-2 gap-0.5 overflow-hidden rounded-full" aria-hidden="true">
				{#each versions as v (v.version)}
					<span
						class="swatch h-full"
						style:--light={v.color[0]}
						style:--dark={v.color[1]}
						style:flex-grow={v.count}
						title="{v.version}: {v.count} {v.count === 1 ? 'device' : 'devices'} ({share(v.count)})"
					></span>
				{/each}
			</span>
			<span class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
				{#each versions as v (v.version)}
					<span class="inline-flex items-center gap-1.5">
						<span
							class="swatch size-2 rounded-full"
							style:--light={v.color[0]}
							style:--dark={v.color[1]}
						></span>
						<span class="font-semibold">{v.version}</span>
						{#if v.version === stats.latest}<span class="text-zinc-400">latest</span>{/if}
						<span class="text-zinc-500 tabular-nums"
							>{counts
								? `${v.count} ${v.count === 1 ? 'device' : 'devices'}`
								: share(v.count)}</span
						>
					</span>
				{/each}
			</span>
		</button>
	</div>
{/if}

<style>
	.swatch {
		background: var(--light);
	}
	@media (prefers-color-scheme: dark) {
		.swatch {
			background: var(--dark);
		}
	}
</style>
