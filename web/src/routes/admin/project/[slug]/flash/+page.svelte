<script lang="ts">
	import { beforeNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { api } from '#lib/api.ts';
	import FlashProgress, { type Line } from '#lib/components/FlashProgress.svelte';
	import { flash, place, tableFromImage } from '#lib/flash.ts';
	import { date, size } from '#lib/format.ts';
	import { connect, hasSerial, reconnect } from '#lib/serial.ts';
	import type { Project, Release } from '#lib/types.ts';

	// A blank (or bricked) board: everything is written, bootloader and partition table included, and
	// the flash is erased first. Its files come through the admin API, so no device key is needed.

	const slug = $derived(page.params.slug ?? '');

	let project = $state<Project | null>(null);
	let releases = $state<Release[]>([]);
	let tag = $state('');
	let error = $state('');

	const STEPS = ['Connect', 'Download', 'Flash', 'Restart', 'Check boot'];
	let step = $state(-1);
	let progress = $state(0);
	let running = $state(false);
	let logs = $state<Line[]>([]);
	let booted = $state('');

	const release = $derived(releases.find((r) => r.tag === tag));
	// "bootloader.bin@0x0,partitions.bin@0x8000", as the server normalised it.
	const extras = $derived(
		(project?.first_flash ?? '')
			.split(',')
			.filter(Boolean)
			.map((entry) => {
				const [name, offset] = entry.split('@');
				return { name, address: parseInt(offset, 16) };
			})
	);

	onMount(async () => {
		try {
			project = (await api.projects()).find((p) => p.slug === slug) ?? null;
			if (!project) throw new Error('No such project.');
			releases = await api.releases(project.id);
			tag = project.latest ?? releases[0]?.tag ?? '';
		} catch (err) {
			error = (err as Error).message;
		}
	});

	function log(text: string, kind?: Line['kind']) {
		for (const line of text.split('\n').filter((l) => l.trim()))
			logs.push({ at: new Date().toLocaleTimeString(), text: line, kind });
	}

	async function install() {
		if (!project || !release) return;
		error = '';
		logs = [];
		booted = '';
		progress = 0;
		running = true;
		step = 0;
		let board: Awaited<ReturnType<typeof connect>> | undefined;
		try {
			// First, while the click still allows the port picker.
			board = await connect();
			const port = board.port;
			await board.close();
			board = undefined;

			step = 1;
			const files: { name: string; data: Uint8Array; address: number }[] = [];
			const assets: { name: string; data: Uint8Array }[] = [];
			for (const asset of release.assets) {
				log(`Downloading ${asset.name} (${size(asset.size)})…`);
				const data = await api.download(project.id, release.tag, asset.name);
				const extra = extras.find((e) => e.name === asset.name);
				if (extra) files.push({ name: asset.name, data, address: extra.address });
				else assets.push({ name: asset.name, data });
			}
			const missing = extras.filter((e) => !files.some((f) => f.name === e.name));
			if (missing.length)
				throw new Error(`${release.tag} has no ${missing.map((e) => e.name).join(', ')}.`);
			const table = files.map((f) => tableFromImage(f.data)).find((t) => t.length) ?? [];
			if (!table.length)
				throw new Error(
					'No first-flash file is a partition table: add partitions.bin@0x8000 on the project.'
				);
			for (const p of table)
				log(`Partition ${p.label} at 0x${p.offset.toString(16)} (${size(p.size)})`);
			const placed = place(table, assets);
			for (const name of placed.skipped) log(`Skipping ${name}: not an app or filesystem image.`);
			files.push(...placed.files.filter((p) => !files.some((f) => f.address === p.address)));

			step = 2;
			log(`Erasing the board and writing ${release.tag}…`);
			await flash(
				port,
				files,
				(line) => log(line),
				(fraction) => {
					progress = fraction;
					if (fraction >= 1) step = 3;
				},
				true
			);
			step = 4;
			log('Waiting for the device to boot…');
			board = await reconnect(port);
			const reply = await board.ask('beacon info', 'beacon:info ', 30000).catch(() => {
				throw new Error(
					'The device came back on USB but did not answer: the firmware may not boot. Check its serial log.'
				);
			});
			booted = reply.split(' ')[1] ?? '';
			log(`The device booted ${booted}.`, 'ok');
			step = STEPS.length;
		} catch (err) {
			if ((err as Error).name === 'NotFoundError') step = -1;
			else {
				error = (err as Error).message;
				log(error, 'error');
			}
		} finally {
			await board?.close();
			running = false;
		}
	}

	beforeNavigate((nav) => {
		if (running && !confirm('The flash is still running. Leave anyway?')) nav.cancel();
	});
</script>

<svelte:head><title>First flash{project ? ` · ${project.name}` : ''} · Beacon</title></svelte:head>
<svelte:window onbeforeunload={(e) => running && e.preventDefault()} />

<div class="mx-auto max-w-3xl space-y-8 px-4 pb-20 sm:px-6">
	<header class="flex items-center gap-4">
		<span
			class="grid size-14 shrink-0 place-items-center rounded-lg bg-amber-100 text-3xl dark:bg-amber-500/15"
			aria-hidden="true">{project?.emoji ?? '📦'}</span
		>
		<div class="min-w-0">
			<h1 class="truncate text-xl font-semibold">First flash</h1>
			<p class="truncate text-xs text-zinc-400">
				Erases a blank or bricked {project?.name ?? ''} board over USB and writes everything.
			</p>
		</div>
	</header>

	{#if !hasSerial()}
		<p class="rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm">
			Flashing talks to the device over USB, which needs Chrome or Edge on a computer.
		</p>
	{/if}
	{#if project && !extras.length}
		<p class="rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm">
			This project has no first-flash files: add the bootloader and partition table under
			<a href="{resolve('/admin')}?project={project.id}" class="underline">Edit</a>.
		</p>
	{/if}
	{#if error && step < 0}
		<p class="text-sm font-semibold text-red-600" role="alert">{error}</p>
	{/if}

	<section class="space-y-4">
		<label class="block space-y-2">
			<span class="text-sm font-semibold text-zinc-500 uppercase">Version</span>
			<select bind:value={tag} disabled={running || !releases.length} class="field">
				{#each releases as r (r.tag)}
					<option value={r.tag}
						>{r.tag}{r.prerelease ? ' (prerelease)' : ''}{r.tag === project?.latest
							? ' (latest)'
							: ''} · {date(r.published_at)}</option
					>
				{:else}
					<option value="">No releases yet</option>
				{/each}
			</select>
		</label>
		{#if release}
			<ul class="space-y-1">
				{#each release.assets as asset (asset.name)}
					{@const extra = extras.find((e) => e.name === asset.name)}
					<li class="flex items-center gap-1.5 text-xs text-zinc-500">
						<span class="size-1.5 shrink-0 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
						<span class="truncate font-medium text-zinc-700 dark:text-zinc-300">{asset.name}</span>
						<span class="shrink-0"
							>· {size(asset.size)}{extra
								? ` · first flash at 0x${extra.address.toString(16)}`
								: ''}</span
						>
					</li>
				{/each}
			</ul>
		{/if}
		<button
			onclick={install}
			disabled={running || !release || !extras.length || !hasSerial()}
			class="w-full cursor-pointer rounded-lg bg-yellow-400 px-5 py-3 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300 disabled:cursor-not-allowed disabled:opacity-50"
			>{running ? 'Flashing…' : `Connect, erase and flash ${tag}`}</button
		>
	</section>

	{#if step >= 0}
		<FlashProgress
			steps={STEPS}
			{step}
			flashStep={2}
			{progress}
			{running}
			{error}
			{logs}
			done="The device runs {booted}. Register it with Sync device on the project page."
		/>
	{/if}
</div>
