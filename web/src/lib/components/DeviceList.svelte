<script lang="ts">
	import { api } from '#lib/api.ts';
	import { ago, day } from '#lib/format.ts';
	import type { Device, DeviceCredentials } from '#lib/types.ts';
	import BarChart from './BarChart.svelte';
	import List from './List.svelte';

	let { projectId, onchange }: { projectId: number; onchange: () => void } = $props();

	let devices = $state<Device[]>([]);
	let error = $state('');
	let name = $state('');
	let created = $state<DeviceCredentials | null>(null);

	async function load() {
		try {
			devices = await api.devices(projectId);
		} catch (err) {
			error = (err as Error).message;
		}
	}
	$effect(() => {
		created = null; // a secret belongs to the project it was made for
		load();
	});

	async function register(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			created = await api.createDevice(projectId, name);
			name = '';
			await load();
			onchange();
		} catch (err) {
			error = (err as Error).message;
		}
	}

	// `activity` ends today; label its days from this browser's clock.
	const today = Math.floor(Date.now() / 86400000);
	const activity = (device: Device) =>
		device.activity.map((value, i, all) => ({
			label: day(today - all.length + 1 + i),
			value
		}));

	async function revoke(device: Device) {
		if (!confirm(`Revoke ${device.name}? It will no longer get updates.`)) return;
		await api.deleteDevice(device.id).catch((err) => (error = err.message));
		await load();
		onchange();
	}
</script>

<h2 class="mb-4 text-sm font-semibold text-zinc-500 uppercase">Devices · {devices.length}</h2>

<form class="mb-4 flex gap-3" onsubmit={register}>
	<input bind:value={name} required maxlength="100" class="field" placeholder="New device name" />
	<button
		class="shrink-0 cursor-pointer rounded-lg bg-yellow-400 px-5 py-2.5 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300"
		>Register</button
	>
</form>

{#if created}
	<!-- jrg-life's notice style (its sign-in "no access" box). -->
	<div
		class="mb-4 space-y-1 rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm"
		role="status"
	>
		<p class="font-semibold">Flash these into the device now: the secret is not shown again.</p>
		<p class="text-xs">Device id <code class="break-all select-all">{created.id}</code></p>
		<p class="text-xs">Secret <code class="break-all select-all">{created.secret}</code></p>
	</div>
{/if}
{#if error}<p class="mb-4 text-sm font-semibold text-red-600" role="alert">{error}</p>{/if}

{#if devices.length}
	<List items={devices} key={(d) => d.id}>
		{#snippet row(device)}
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1 py-3 pr-2 pl-4">
				<div class="min-w-0 flex-1">
					<p class="flex items-center gap-2 truncate font-semibold">
						{device.name}
						<span
							class="inline-flex items-center gap-1.5 rounded-full border border-zinc-200 px-2 py-0.5 text-[11px] font-medium text-zinc-500 dark:border-zinc-700"
						>
							<span
								class="size-1.5 rounded-full {device.up_to_date
									? 'bg-emerald-500'
									: device.last_version
										? 'bg-yellow-500'
										: 'bg-zinc-300 dark:bg-zinc-600'}"
							></span>
							{device.last_version ?? 'no version yet'}{device.last_version && !device.up_to_date
								? ' · update pending'
								: ''}
						</span>
					</p>
					<p class="truncate text-xs text-zinc-400">
						{device.id} · seen {ago(device.last_seen)} · {device.checks} checks · {device.downloads}
						downloads
					</p>
				</div>
				<div class="w-24 shrink-0">
					<BarChart title="{device.name}: requests per day" data={activity(device)} />
				</div>
				<button class="action action-danger" onclick={() => revoke(device)}>Revoke</button>
			</div>
		{/snippet}
	</List>
{:else}
	<p class="text-xs text-zinc-500">No devices yet: register one, then flash its id and secret.</p>
{/if}
