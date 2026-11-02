<script lang="ts">
	import { api, register } from '#lib/api.ts';
	import { ago, day } from '#lib/format.ts';
	import { connect, hasSerial } from '#lib/serial.ts';
	import type { Device } from '#lib/types.ts';
	import BarChart from './BarChart.svelte';
	import List from './List.svelte';

	let {
		projectId,
		isPublic,
		onchange
	}: { projectId: number; isPublic: boolean; onchange: () => void } = $props();

	let devices = $state<Device[]>([]);
	let error = $state('');
	let name = $state('');
	let synced = $state<{ name: string; id: string } | null>(null);
	let syncing = $state(false);

	async function load() {
		try {
			devices = await api.devices(projectId);
		} catch (err) {
			error = (err as Error).message;
		}
	}
	$effect(() => {
		synced = null; // it belongs to the project it was made for
		load();
	});

	async function sync(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		synced = null;
		syncing = true;
		let board: Awaited<ReturnType<typeof connect>> | undefined;
		try {
			board = await connect();
			const id = await register(board, projectId, name).finally(() => {
				load();
				onchange();
			});
			synced = { name, id };
			name = '';
		} catch (err) {
			// Closing the port picker is not an error.
			if ((err as Error).name !== 'NotFoundError') error = (err as Error).message;
		} finally {
			await board?.close();
			syncing = false;
		}
	}

	// `activity` ends today; label its days from this browser's clock.
	const today = Math.floor(Date.now() / 86400000);
	const activity = (device: Device) =>
		device.activity.map((value, i, all) => ({
			label: day(today - all.length + 1 + i),
			value
		}));

	// Devices report their version only when they check, so `pending` lasts until the next check.
	const UPDATE = {
		pending: { dot: 'bg-yellow-500', label: 'waiting for the device to report back' },
		succeeded: { dot: 'bg-emerald-500', label: 'installed' },
		failed: { dot: 'bg-red-500', label: 'failed, still on the old version' }
	};

	// The device being renamed, and the name typed so far.
	let renaming = $state<{ id: string; name: string } | null>(null);

	async function rename(e: SubmitEvent) {
		e.preventDefault();
		if (!renaming) return;
		try {
			await api.renameDevice(renaming.id, renaming.name);
			renaming = null;
			await load();
		} catch (err) {
			error = (err as Error).message;
		}
	}

	async function revoke(device: Device) {
		if (!confirm(`Revoke ${device.name}? It will no longer get updates.`)) return;
		await api.deleteDevice(device.id).catch((err) => (error = err.message));
		await load();
		onchange();
	}
</script>

<h2 class="mb-4 text-sm font-semibold text-zinc-500 uppercase">Devices · {devices.length}</h2>

{#if isPublic}
	<p class="mb-4 text-xs text-zinc-500">
		A public project's devices need no sync: each gets its id on its first update check or first
		flash.
	</p>
{:else}
	<form class="mb-4 flex gap-3" onsubmit={sync}>
		<input bind:value={name} required maxlength="100" class="field" placeholder="New device name" />
		<button
			disabled={syncing || !hasSerial()}
			class="shrink-0 cursor-pointer rounded-lg bg-yellow-400 px-5 py-2.5 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300 disabled:cursor-not-allowed disabled:opacity-50"
			>{syncing ? 'Syncing…' : 'Sync device'}</button
		>
	</form>
	{#if !hasSerial()}
		<p class="mb-4 text-xs text-zinc-500">
			Syncing talks to the device over USB, which needs Chrome or Edge on a computer.
		</p>
	{/if}
{/if}

{#if synced}
	<p class="mb-4 text-sm text-emerald-600" role="status">
		{synced.name} is registered as <code>{synced.id}</code>. Its private key stays on the device.
	</p>
{/if}
{#if error}<p class="mb-4 text-sm font-semibold text-red-600" role="alert">{error}</p>{/if}

{#if devices.length}
	<List items={devices} key={(d) => d.id} pageSize={10}>
		{#snippet row(device)}
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1 py-3 pr-2 pl-4">
				<div class="min-w-0 flex-1">
					<!-- The version pill wraps under a long name instead of being cut off. -->
					<div class="flex flex-wrap items-center gap-x-2 gap-y-1 font-semibold">
						{#if renaming?.id === device.id}
							<form class="flex min-w-0 gap-2" onsubmit={rename}>
								<!-- svelte-ignore a11y_autofocus -->
								<input
									bind:value={renaming.name}
									required
									maxlength="100"
									autofocus
									aria-label="Device name"
									class="field py-1"
									onkeydown={(e) => e.key === 'Escape' && (renaming = null)}
								/>
								<button class="action">Save</button>
								<button type="button" class="action" onclick={() => (renaming = null)}
									>Cancel</button
								>
							</form>
						{:else}
							<span class="min-w-0 truncate">{device.name}</span>
						{/if}
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
					</div>
					<p class="truncate text-xs text-zinc-400">
						{device.id} · seen {ago(device.last_seen)} · {device.checks} checks · {device.downloads}
						downloads
					</p>
					{#if device.update}
						{@const update = UPDATE[device.update.status]}
						<p class="flex min-w-0 items-center gap-1.5 text-xs text-zinc-500">
							<span class="size-1.5 shrink-0 rounded-full {update.dot}"></span>
							<span class="truncate"
								>Update to {device.update.tag}: {update.label} · downloaded {ago(
									device.update.at
								)}</span
							>
						</p>
					{/if}
				</div>
				<!-- Too cramped beside the details on a phone; the project stats chart the same traffic. -->
				<div class="hidden w-24 shrink-0 sm:block">
					<BarChart title="{device.name}: requests per day" data={activity(device)} />
				</div>
				<button class="action" onclick={() => (renaming = { id: device.id, name: device.name })}
					>Rename</button
				>
				<button class="action action-danger" onclick={() => revoke(device)}>Revoke</button>
			</div>
		{/snippet}
	</List>
{:else}
	<p class="text-xs text-zinc-500">No devices yet: plug one in over USB, name it and sync it.</p>
{/if}
