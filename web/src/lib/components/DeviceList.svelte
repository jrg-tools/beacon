<script lang="ts">
	import { api } from '#lib/api.ts';
	import { ago } from '#lib/format.ts';
	import type { Device, DeviceCredentials } from '#lib/types.ts';
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
		class="shrink-0 cursor-pointer rounded-full bg-yellow-400 px-5 py-2.5 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300"
		>Register</button
	>
</form>

{#if created}
	<!-- jrg-life's notice style (its sign-in "no access" box). -->
	<div
		class="mb-4 space-y-1 rounded-xl border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm"
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
					<p class="truncate font-semibold">{device.name}</p>
					<p class="truncate text-xs text-zinc-400">
						{device.id} · {device.last_version ?? 'no version yet'} · seen {ago(device.last_seen)}
					</p>
				</div>
				<button class="action action-danger" onclick={() => revoke(device)}>Revoke</button>
			</div>
		{/snippet}
	</List>
{:else}
	<p class="text-xs text-zinc-500">No devices yet: register one, then flash its id and secret.</p>
{/if}
