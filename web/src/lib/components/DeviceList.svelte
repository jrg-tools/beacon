<script lang="ts">
	import { api } from '#lib/api.ts';
	import { ago, day } from '#lib/format.ts';
	import { connect, hasSerial } from '#lib/serial.ts';
	import type { Device } from '#lib/types.ts';
	import BarChart from './BarChart.svelte';
	import List from './List.svelte';

	let { projectId, onchange }: { projectId: number; onchange: () => void } = $props();

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

	// The board makes its own key pair and hands over only the public key, so the private key never
	// leaves it. Each sync gets a fresh nonce, so resending keygen can't swap the key mid-sync.
	async function sync(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		synced = null;
		syncing = true;
		let board: Awaited<ReturnType<typeof connect>> | undefined;
		try {
			board = await connect();
			const nonce = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) =>
				b.toString(16).padStart(2, '0')
			).join('');
			const publicKey = await board.ask(`beacon keygen ${nonce}`, `beacon:pubkey ${nonce} `);
			const { id } = await api.createDevice(projectId, name, publicKey);
			await load();
			onchange();
			await board.ask(`beacon id ${id}`, `beacon:ok ${id}`).catch(() => {
				throw new Error(
					`Registered ${id}, but the device did not save it: revoke it and sync again.`
				);
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

	async function revoke(device: Device) {
		if (!confirm(`Revoke ${device.name}? It will no longer get updates.`)) return;
		await api.deleteDevice(device.id).catch((err) => (error = err.message));
		await load();
		onchange();
	}
</script>

<h2 class="mb-4 text-sm font-semibold text-zinc-500 uppercase">Devices · {devices.length}</h2>

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

{#if synced}
	<p class="mb-4 text-sm text-emerald-600" role="status">
		{synced.name} is registered as <code>{synced.id}</code>. Its private key stays on the device.
	</p>
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
				<div class="w-24 shrink-0">
					<BarChart title="{device.name}: requests per day" data={activity(device)} />
				</div>
				<button class="action action-danger" onclick={() => revoke(device)}>Revoke</button>
			</div>
		{/snippet}
	</List>
{:else}
	<p class="text-xs text-zinc-500">No devices yet: plug one in over USB, name it and sync it.</p>
{/if}
