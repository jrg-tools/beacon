<script lang="ts">
	import { beforeNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onMount, tick } from 'svelte';
	import { api } from '#lib/api.ts';
	import { flash, partitions } from '#lib/flash.ts';
	import { date, size } from '#lib/format.ts';
	import { connect, hasSerial } from '#lib/serial.ts';
	import type { Project, Release } from '#lib/types.ts';

	const slug = $derived(page.params.slug ?? '');

	let project = $state<Project | null>(null);
	const id = $derived(project?.id ?? 0);
	let releases = $state<Release[]>([]);
	let tag = $state('');
	let error = $state('');

	const STEPS = ['Verify device', 'Download', 'Flash', 'Restart'];
	// -1 before a run; STEPS.length once it is done.
	let step = $state(-1);
	let progress = $state(0);
	let running = $state(false);
	let device = $state<{ id: string; name: string } | null>(null);
	let logs = $state<{ at: string; text: string; kind?: 'error' | 'ok' }[]>([]);
	let terminal = $state<HTMLElement>();

	const release = $derived(releases.find((r) => r.tag === tag));

	onMount(async () => {
		try {
			project = (await api.projects()).find((p) => p.slug === slug) ?? null;
			releases = project ? await api.releases(project.id) : [];
			tag = project?.latest ?? releases[0]?.tag ?? '';
			if (!project) error = 'No such project.';
		} catch (err) {
			error = (err as Error).message;
		}
	});

	async function log(text: string, kind?: 'error' | 'ok') {
		for (const line of text.split('\n').filter((l) => l.trim()))
			logs.push({ at: new Date().toLocaleTimeString(), text: line, kind });
		await tick();
		terminal?.scrollTo({ top: terminal.scrollHeight });
	}

	// The board must prove, with the key it made when it was synced, that it is a device of this
	// project before anything is written to it.
	async function install() {
		if (!release) return;
		error = '';
		logs = [];
		device = null;
		progress = 0;
		running = true;
		step = 0;
		let board: Awaited<ReturnType<typeof connect>> | undefined;
		try {
			board = await connect();
			log('Port open, asking the device who it is…');
			const nonce = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) =>
				b.toString(16).padStart(2, '0')
			).join('');
			const reply = await board
				.ask(`beacon whoami ${nonce}`, `beacon:whoami ${nonce} `)
				.catch(() => {
					throw new Error(
						'The device did not answer "beacon whoami": its firmware predates installs from here, so update it over the air once.'
					);
				});
			const [claimed, signature] = reply.split(' ');
			if (!signature)
				throw new Error('This device is not registered: sync it on the project page first.');
			device = await api.verifyDevice(id, { device: claimed, nonce, signature });
			log(`Verified ${device.name} (${device.id}).`, 'ok');
			const table = partitions(
				await board.ask('beacon partitions', 'beacon:partitions ', 5000).catch(() => {
					throw new Error(
						'The device did not report its partitions: its firmware predates installs from here, so update it over the air or with PlatformIO once.'
					);
				})
			);
			if (!table.length) throw new Error('The device reported no partitions.');
			for (const p of table)
				log(`Partition ${p.label} at 0x${p.offset.toString(16)} (${size(p.size)})`);
			const port = board.port;
			await board.close();
			board = undefined;

			step = 1;
			const assets = [];
			for (const asset of release.assets) {
				log(`Downloading ${asset.name} (${size(asset.size)})…`);
				assets.push({ name: asset.name, data: await api.asset(id, release.tag, asset.name) });
			}

			step = 2;
			log(`Entering the bootloader to install ${release.tag}…`);
			await flash(
				port,
				table,
				assets,
				(line) => log(line),
				(fraction) => {
					progress = fraction;
					if (fraction >= 1) step = 3;
				}
			);
			step = STEPS.length;
			log(`${device.name} now runs ${release.tag}.`, 'ok');
		} catch (err) {
			// Closing the port picker is not an error.
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

	// Leaving mid-flash would leave half an app on the board.
	beforeNavigate((nav) => {
		if (running && !confirm('The install is still running. Leave anyway?')) nav.cancel();
	});
</script>

<svelte:head><title>Install{project ? ` · ${project.name}` : ''} · Beacon</title></svelte:head>
<svelte:window onbeforeunload={(e) => running && e.preventDefault()} />

<div class="mx-auto max-w-3xl space-y-8 px-4 pb-20 sm:px-6">
	<div class="space-y-3">
		<a
			href={resolve('/admin/project/[slug]', { slug })}
			class="-ml-3 inline-block rounded-lg px-3 py-1.5 text-sm font-semibold text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
			>← {project?.name ?? 'Project'}</a
		>
		<header class="flex items-center gap-4">
			<span
				class="grid size-14 shrink-0 place-items-center rounded-lg bg-amber-100 text-3xl dark:bg-amber-500/15"
				aria-hidden="true">{project?.emoji ?? '📦'}</span
			>
			<div class="min-w-0">
				<h1 class="truncate text-xl font-semibold">Install firmware</h1>
				<p class="truncate text-xs text-zinc-400">
					Over USB, onto a registered {project?.name ?? ''} device. Its settings and key stay.
				</p>
			</div>
		</header>
	</div>

	{#if !hasSerial()}
		<p class="rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm">
			Installing talks to the device over USB, which needs Chrome or Edge on a computer.
		</p>
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
					<li class="flex items-center gap-1.5 text-xs text-zinc-500">
						<span class="size-1.5 shrink-0 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
						<span class="truncate font-medium text-zinc-700 dark:text-zinc-300">{asset.name}</span>
						<span class="shrink-0">· {size(asset.size)}</span>
					</li>
				{/each}
			</ul>
		{/if}
		<button
			onclick={install}
			disabled={running || !release || !hasSerial()}
			class="w-full cursor-pointer rounded-lg bg-yellow-400 px-5 py-3 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300 disabled:cursor-not-allowed disabled:opacity-50"
			>{running ? 'Installing…' : `Connect and install ${tag}`}</button
		>
	</section>

	{#if step >= 0}
		<section class="space-y-5" aria-live="polite">
			<ol class="grid grid-cols-4 gap-2">
				{#each STEPS as label, i (label)}
					{@const state =
						i < step ? 'done' : i === step ? (error ? 'failed' : 'active') : 'waiting'}
					<li class="space-y-2">
						<div
							class="h-1.5 overflow-hidden rounded-full {state === 'done'
								? 'bg-emerald-500'
								: state === 'failed'
									? 'bg-red-500'
									: 'bg-zinc-200 dark:bg-zinc-800'}"
						>
							{#if state === 'active'}<div
									class="sweep h-full w-1/2 rounded-full bg-yellow-400"
								></div>{/if}
						</div>
						<p
							class="flex items-center gap-1.5 text-xs font-semibold {state === 'waiting'
								? 'text-zinc-400'
								: ''}"
						>
							{#if state === 'active'}
								<span
									class="size-3 shrink-0 animate-spin rounded-full border-2 border-yellow-400 border-t-transparent"
								></span>
							{:else if state === 'done'}
								<span class="text-emerald-500">✓</span>
							{:else if state === 'failed'}
								<span class="text-red-500">✕</span>
							{/if}
							<span class="truncate">{label}</span>
						</p>
					</li>
				{/each}
			</ol>

			{#if step >= 2}
				<div class="space-y-1.5">
					<div class="flex justify-between text-xs font-semibold text-zinc-500">
						<span>{step >= STEPS.length ? 'Installed' : 'Writing flash'}</span>
						<span class="tabular-nums">{Math.round(progress * 100)}%</span>
					</div>
					<div
						class="h-3 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-800"
						role="progressbar"
						aria-valuenow={Math.round(progress * 100)}
						aria-valuemin={0}
						aria-valuemax={100}
					>
						<div
							class="stripes h-full rounded-full transition-[width] duration-300 {error
								? 'bg-red-500'
								: progress >= 1
									? 'bg-emerald-500'
									: 'bg-yellow-400'}"
							style="width: {progress * 100}%"
						></div>
					</div>
				</div>
			{/if}

			{#if step >= STEPS.length && device && release}
				<p
					class="rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-4 py-3 text-sm"
					role="status"
				>
					{device.name} is on {release.tag}. It reports the new version on its next update check.
				</p>
			{/if}
			{#if error}
				<p
					class="rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm font-semibold text-red-600"
					role="alert"
				>
					{error}
				</p>
			{/if}

			<div class="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-950">
				<p
					class="flex items-center gap-1.5 border-b border-zinc-800 px-4 py-2 text-[11px] font-semibold text-zinc-500 uppercase"
				>
					<span
						class="size-1.5 rounded-full {running ? 'animate-pulse bg-yellow-400' : 'bg-zinc-600'}"
					></span>
					Log
				</p>
				<div
					bind:this={terminal}
					class="h-72 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed text-zinc-300"
				>
					{#each logs as line, i (i)}
						<p class="break-words whitespace-pre-wrap">
							<span class="text-zinc-600">{line.at}</span>
							<span
								class={line.kind === 'error'
									? 'text-red-400'
									: line.kind === 'ok'
										? 'text-emerald-400'
										: ''}>{line.text}</span
							>
						</p>
					{/each}
					{#if running}<span class="animate-pulse text-yellow-400">▍</span>{/if}
				</div>
			</div>
		</section>
	{/if}
</div>

<style>
	/* The active step's bar sweeps back and forth: busy, with no percentage to show. */
	.sweep {
		animation: sweep 1.2s ease-in-out infinite alternate;
	}
	@keyframes sweep {
		from {
			transform: translateX(-60%);
		}
		to {
			transform: translateX(160%);
		}
	}
	/* Moving stripes on the flash progress bar. */
	.stripes {
		background-image: linear-gradient(
			45deg,
			rgb(255 255 255 / 0.25) 25%,
			transparent 25%,
			transparent 50%,
			rgb(255 255 255 / 0.25) 50%,
			rgb(255 255 255 / 0.25) 75%,
			transparent 75%
		);
		background-size: 1rem 1rem;
		animation: stripes 0.6s linear infinite;
	}
	@keyframes stripes {
		to {
			background-position: 1rem 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.sweep,
		.stripes {
			animation: none;
		}
	}
</style>
