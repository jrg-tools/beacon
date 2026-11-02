<script lang="ts">
	import { beforeNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { ClerkProvider } from 'svelte-clerk/client';
	import { ApiError, api, deviceGet, fetchConfig, projectReleases, register } from '#lib/api.ts';
	import AdminSignIn from '#lib/components/AdminSignIn.svelte';
	import ConnectHint from '#lib/components/ConnectHint.svelte';
	import FlashProgress, { type Line } from '#lib/components/FlashProgress.svelte';
	import { extras, firstFlashFiles, flash, partitions, place } from '#lib/flash.ts';
	import { date, size } from '#lib/format.ts';
	import { connect, hasSerial, reconnect } from '#lib/serial.ts';
	import type { InstallProject, Project, Release } from '#lib/types.ts';

	const slug = $derived(page.params.slug ?? '');

	let project = $state<InstallProject | null>(null);
	// A 404: a private project (or none) for anyone but its admin. The board's signed `latest` then
	// says which release it gets, so nothing about the project shows before the board proves itself.
	let hidden = $state(false);
	// The admin sign-in, loaded only once asked for: its Clerk key.
	let signIn = $state<Promise<{ clerk_publishable_key: string }>>();
	// Signed in as an admin: the project as the admin API has it, with every release file.
	let admin = $state<{ project: Project; releases: Release[] } | null>(null);
	// The 5 newest (the server sorts by date) and the current release, which may be older than them.
	const releases = $derived(
		(admin?.releases ?? project?.releases ?? []).filter(
			(r, i) => i < 5 || r.tag === project?.latest
		)
	);
	let tag = $state('');
	let error = $state('');
	// Bootloader, partition table…: a public project's are open, a private one's only its admin's.
	const flashExtras = $derived(extras(admin?.project.first_flash ?? project?.first_flash ?? ''));
	// Private projects only: a public one detects a board that needs it.
	let erase = $state(false);
	const canErase = $derived(!!admin && !project?.public && flashExtras.length > 0);

	const STEPS = ['Verify device', 'Download', 'Flash', 'Restart', 'Check boot'];
	const FIRST_STEPS = [
		'Connect',
		'Download',
		'Erase and flash',
		'Restart',
		'Check boot',
		'Register'
	];
	let steps = $state(STEPS);
	// -1 before a run; steps.length once it is done.
	let step = $state(-1);
	let progress = $state(0);
	let running = $state(false);
	let done = $state('');
	let logs = $state<Line[]>([]);
	// The browser's port picker is open.
	let picking = $state(false);
	// The release going on, once known: a private project's page learns it from the board.
	let installing = $state('');
	// ConnectHint following the run: the port popup, verifying, installing, done.
	const live = $derived(
		step < 0
			? null
			: {
					scene: picking ? 1 : step >= steps.length ? 4 : step === 0 ? 2 : 3,
					progress,
					failed: !!error && !running,
					tag: installing
				}
	);

	const release = $derived(releases.find((r) => r.tag === tag));

	async function load() {
		try {
			project = await projectReleases(slug);
			hidden = false;
			tag = project.latest ?? project.releases[0]?.tag ?? '';
		} catch (err) {
			if (err instanceof ApiError && err.status === 404) hidden = true;
			else error = (err as Error).message;
		}
	}

	onMount(() => {
		load();
		// Coming from the admin pages, Clerk is already loaded and signed in.
		if (window.Clerk?.session) signIn = fetchConfig();
	});

	// Signed in as this project's admin: the full page, the private project's included.
	async function onadmin(found: NonNullable<typeof admin>) {
		admin = found;
		if (!project) await load(); // now with the admin's session
	}

	function log(text: string, kind?: Line['kind']) {
		for (const line of text.split('\n').filter((l) => l.trim()))
			logs.push({ at: new Date().toLocaleTimeString(), text: line, kind });
	}

	type Board = Awaited<ReturnType<typeof connect>>;
	// The open session, closed when a run ends however it ends.
	let board: Board | undefined;

	const NOT_REGISTERED = 'This device is not registered: sync it on the project page first.';

	// Who the board is ("none" before it has an id) and what it runs.
	async function info(timeoutMs: number, silent: string) {
		const reply = await board!.ask('beacon info', 'beacon:info ', timeoutMs).catch(() => {
			throw new Error(silent);
		});
		const [id, version] = reply.split(' ');
		return { id, version };
	}

	const assetPath = (tag: string, name: string) =>
		`/api/v1/firmware/${slug}/releases/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;

	// The device API, as if the board called it. Private: it signs the request over USB with its key,
	// and the server refuses a board that is not a registered device of this project. Public: its id.
	async function boardGet(id: string, path: string) {
		if (project?.public)
			return deviceGet(path, { device: id }).catch((err) => {
				if (err.status === 401) throw new Error(`Beacon does not know ${id}.`);
				throw err;
			});
		const timestamp = Math.floor(Date.now() / 1000);
		const request = `${timestamp} GET ${path}`;
		const signature = await board!.ask(`beacon sign ${request}`, `beacon:sig ${request} `, 5000);
		if (signature === 'none') throw new Error(NOT_REGISTERED);
		return deviceGet(path, { device: id, timestamp, signature }).catch((err) => {
			if (err.status === 401) throw new Error(NOT_REGISTERED);
			throw err;
		});
	}

	// Also how Beacon learns the version: the same check the board makes over WiFi.
	const check = (id: string, version: string) =>
		boardGet(id, `/api/v1/firmware/${slug}/latest?current=${encodeURIComponent(version)}`);

	const sameVersion = (a: string, b: string) => a.replace(/^v/, '') === b.replace(/^v/, '');

	const flashing = (fraction: number) => {
		progress = fraction;
		if (fraction >= 1) step = 3;
	};

	type Target = { tag: string; assets: { name: string; size: number }[] };

	// Verified before anything is written, and again once the new firmware has booted. NVS stays.
	// Without `rel` (a private project's page for a non-admin), the release is whatever the board's
	// own `latest` offers it.
	async function update(rel: Target | undefined, before: { id: string; version: string }) {
		if (before.id === 'none') throw new Error(NOT_REGISTERED);
		const latest = await (await check(before.id, before.version)).json();
		log(`Your device is verified (${before.id}), on ${before.version}.`, 'ok');
		if (!rel) {
			if (!latest.update_available) {
				done = `Your device is on ${before.version}: no newer release.`;
				return;
			}
			rel = latest.latest as Target;
			log(`Its newest release is ${rel.tag}.`);
		}
		installing = rel.tag;
		const table = partitions(await board!.ask('beacon partitions', 'beacon:partitions ', 5000));
		if (!table.length) throw new Error('The device reported no partitions.');
		for (const p of table)
			log(`Partition ${p.label} at 0x${p.offset.toString(16)} (${size(p.size)})`);

		// While the board is still connected: it signs each download.
		step = 1;
		const assets = [];
		for (const asset of rel.assets) {
			if (flashExtras.some((e) => e.name === asset.name)) continue;
			log(`Downloading ${asset.name} (${size(asset.size)})…`);
			const res = await boardGet(before.id, assetPath(rel.tag, asset.name));
			assets.push({ name: asset.name, data: new Uint8Array(await res.arrayBuffer()) });
		}
		const port = board!.port;
		await board!.close();
		board = undefined;

		step = 2;
		const { files, skipped } = place(table, assets);
		for (const name of skipped) log(`Skipping ${name}: not an app or filesystem image.`);
		log(`Entering the bootloader to install ${rel.tag}…`);
		await flash(port, files, (line) => log(line), flashing);
		step = 4;
		log('Waiting for the device to boot…');
		board = await reconnect(port);
		const after = await info(
			30000,
			'The device came back on USB but did not answer: the new firmware may not boot. Install it again or check its serial log.'
		);
		if (after.id !== before.id)
			throw new Error(`A different device answered after the restart (${after.id}).`);
		if (!sameVersion(after.version, rel.tag))
			throw new Error(`It booted ${after.version}, not ${rel.tag}.`);
		await check(after.id, after.version);
		log(`Your device booted ${after.version}, key intact.`, 'ok');
		done = `Your device is on ${rel.tag}.`;
	}

	// A blank or bricked board: the flash is erased and everything written, bootloader and partition
	// table included. Then it gets its id: issued by Beacon (public) or registered with a key (private).
	async function firstFlash(rel: NonNullable<typeof release>) {
		steps = FIRST_STEPS;
		const port = board!.port;
		await board!.close();
		board = undefined;

		step = 1;
		let id = '';
		if (project?.public) {
			// Its first check, made for it: the id carries the downloads and is written once it boots.
			const res = await deviceGet(`/api/v1/firmware/${slug}/latest`, {});
			id = (await res.json()).device;
			log(`Beacon gave the device the id ${id}.`);
		}
		const downloaded = [];
		for (const asset of rel.assets) {
			log(`Downloading ${asset.name} (${size(asset.size)})…`);
			const data =
				admin && !project?.public
					? await api.download(admin.project.id, rel.tag, asset.name)
					: new Uint8Array(
							await (await boardGet(id, assetPath(rel.tag, asset.name))).arrayBuffer()
						);
			downloaded.push({ name: asset.name, data });
		}
		const { files, table, skipped } = firstFlashFiles(flashExtras, downloaded, rel.tag);
		for (const p of table)
			log(`Partition ${p.label} at 0x${p.offset.toString(16)} (${size(p.size)})`);
		for (const name of skipped) log(`Skipping ${name}: not an app or filesystem image.`);

		step = 2;
		log(`Erasing the board and writing ${rel.tag}…`);
		await flash(port, files, (line) => log(line), flashing, true);
		step = 4;
		log('Waiting for the device to boot…');
		board = await reconnect(port);
		const { version } = await info(
			30000,
			'The device came back on USB but did not answer: the firmware may not boot. Check its serial log.'
		);
		log(`The device booted ${version}.`, 'ok');

		step = 5;
		if (project?.public)
			await board.ask(`beacon id ${id}`, `beacon:ok ${id}`).catch(() => {
				throw new Error(`The device did not save its id ${id}: flash it again.`);
			});
		else {
			const name = await askName();
			if (!name)
				throw new Error(
					`The device runs ${version} but is not registered: sync it on the project page.`
				);
			id = await register(board, admin!.project.id, name);
		}
		await check(id, version);
		log(`Your device is ${id}.`, 'ok');
		done = `Your device runs ${version} as ${id}${project?.public ? '' : ', its key made on the board'}.`;
	}

	async function install() {
		if (!hidden && (!project || !release)) return;
		error = '';
		logs = [];
		done = '';
		progress = 0;
		steps = STEPS;
		running = true;
		installing = '';
		step = 0;
		try {
			// First, while the click still allows the port picker.
			picking = true;
			board = await connect();
			picking = false;
			if (canErase && erase) await firstFlash(release!);
			else {
				log('Port open, asking the device who it is…');
				// A public project's board that doesn't answer runs no (or foreign) firmware.
				const before = await info(
					project?.public ? 10000 : 15000,
					'The device did not answer "beacon info": its firmware predates installs from here, so update it over the air or with PlatformIO once.'
				).catch((err) => {
					if (project?.public) return { id: 'none', version: '' };
					throw err;
				});
				if (project?.public && before.id === 'none') {
					log(
						before.version
							? 'The device has no id yet: first flash.'
							: 'No answer to "beacon info" (a blank board or other firmware): first flash.'
					);
					// Anyone's board may be plugged in: never wipe it without asking.
					if ((await ask(eraseDialog!)) !== 'erase')
						throw new Error('Cancelled: the board was not touched.');
					await firstFlash(release!);
				} else await update(release, before);
			}
			step = steps.length;
		} catch (err) {
			// Closing the port picker is not an error.
			if ((err as Error).name === 'NotFoundError') step = -1;
			else {
				error = (err as Error).message;
				log(error, 'error');
			}
		} finally {
			picking = false;
			await board?.close();
			board = undefined;
			running = false;
		}
	}

	// A first flash's last step on a private project: Sync device's name, asked in the page.
	let dialog = $state<HTMLDialogElement>();
	let deviceName = $state('');
	// A public project's first flash erases the board: confirmed first.
	let eraseDialog = $state<HTMLDialogElement>();

	// Opens `d`; its form's submit button value once it closes ('' if dismissed).
	function ask(d: HTMLDialogElement) {
		d.returnValue = '';
		d.showModal();
		return new Promise<string>((resolve) =>
			d.addEventListener('close', () => resolve(d.returnValue), { once: true })
		);
	}

	// The name typed, or '' if the dialog was dismissed.
	async function askName() {
		deviceName = '';
		return (await ask(dialog!)) === 'register' ? deviceName.trim() : '';
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
		<header class="flex items-center gap-4">
			<span
				class="grid size-14 shrink-0 place-items-center rounded-lg border border-zinc-200 bg-zinc-50 text-zinc-500 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-400"
				aria-hidden="true"
			>
				<svg
					viewBox="0 0 24 24"
					class="size-7"
					fill="none"
					stroke="currentColor"
					stroke-width="1.5"
					stroke-linecap="round"
					stroke-linejoin="round"
				>
					<rect x="6" y="6" width="12" height="12" rx="2" />
					<rect x="9.5" y="9.5" width="5" height="5" rx="0.5" />
					<path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" />
				</svg>
			</span>
			<div class="min-w-0">
				<h1 class="truncate text-xl font-semibold">
					{hidden ? 'Update your device' : `Install ${project?.name ?? 'firmware'}`}
				</h1>
				<p class="text-sm text-zinc-500 dark:text-zinc-400">
					{#if hidden}
						Connect it with a USB cable to install the latest release. Your settings are kept.
					{:else if project?.public}
						Connect your board with a USB cable to install the latest firmware. Blank boards work
						too.
					{:else}
						Connect your registered device with a USB cable to install the latest release. Your
						settings are kept.
					{/if}
				</p>
			</div>
		</header>
	</div>

	{#if !hasSerial()}
		<p class="rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm">
			Installing talks to the device over USB, which needs Chrome or Edge on a computer.
		</p>
	{/if}
	{#if error && step < 0}
		<p class="text-sm font-semibold text-red-600" role="alert">{error}</p>
	{/if}
	{#if hidden && !admin}
		<ConnectHint {live} />
	{/if}

	<section class="space-y-4">
		{#if !hidden}
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
						{@const extra = flashExtras.find((e) => e.name === asset.name)}
						<li class="flex items-center gap-1.5 text-xs text-zinc-500">
							<span class="size-1.5 shrink-0 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
							<span class="truncate font-medium text-zinc-700 dark:text-zinc-300">{asset.name}</span
							>
							<span class="shrink-0"
								>· {size(asset.size)}{extra
									? ` · first flash at 0x${extra.address.toString(16)}`
									: ''}</span
							>
						</li>
					{/each}
				</ul>
			{/if}
		{/if}
		{#if canErase}
			<label class="flex items-start gap-3">
				<input
					type="checkbox"
					bind:checked={erase}
					disabled={running}
					class="mt-1 size-4 accent-yellow-400"
				/>
				<span class="space-y-0.5">
					<span class="block text-sm font-semibold">First flash (erase everything)</span>
					<span class="block text-xs text-zinc-500">
						For a blank or bricked board: writes the bootloader and partition table too, then
						registers it. Unchecked: update firmware, keeping its settings and key.
					</span>
				</span>
			</label>
		{/if}
		<button
			onclick={install}
			disabled={running || (!hidden && !release) || !hasSerial()}
			class="w-full cursor-pointer rounded-lg bg-yellow-400 px-5 py-3 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300 disabled:cursor-not-allowed disabled:opacity-50"
			>{running
				? 'Installing…'
				: hidden
					? 'Connect and update'
					: canErase && erase
						? `Connect, erase and flash ${tag}`
						: `Connect and install ${tag}`}</button
		>
		{#if (hidden || project?.public === false) && !admin}
			{#if signIn}
				{#await signIn then { clerk_publishable_key }}
					<ClerkProvider publishableKey={clerk_publishable_key}>
						<AdminSignIn {slug} {onadmin} />
					</ClerkProvider>
				{:catch err}
					<p class="text-sm font-semibold text-red-600" role="alert">{err.message}</p>
				{/await}
			{:else}
				<button
					onclick={() => (signIn = fetchConfig())}
					disabled={running}
					class="cursor-pointer text-xs font-semibold text-zinc-500 underline-offset-2 hover:underline disabled:cursor-not-allowed"
					>Admin sign-in (versions and first flash)</button
				>
			{/if}
		{/if}
	</section>

	{#if step >= 0}
		<FlashProgress {steps} {step} flashStep={2} {progress} {running} {error} {logs} {done} />
	{/if}
</div>

<dialog
	bind:this={dialog}
	class="m-auto w-[calc(100%-2rem)] max-w-md rounded-lg border border-zinc-200 bg-white text-black shadow-xl backdrop:bg-zinc-950/60 backdrop:backdrop-blur-sm dark:border-zinc-800 dark:bg-zinc-900 dark:text-white"
	aria-labelledby="name-title"
>
	<form method="dialog" class="space-y-4 p-6">
		<div class="space-y-1">
			<h2 id="name-title" class="text-lg font-semibold">Name this device</h2>
			<p class="text-xs text-zinc-500">
				It booted. Beacon registers it under this name; the board makes its own key and keeps it.
			</p>
		</div>
		<input bind:value={deviceName} required maxlength="100" class="field" placeholder="Desk" />
		<div class="flex justify-end gap-3">
			<button
				type="button"
				onclick={() => dialog?.close('')}
				class="cursor-pointer rounded-lg px-5 py-2.5 text-sm font-semibold text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
				>Skip</button
			>
			<button
				value="register"
				class="cursor-pointer rounded-lg bg-yellow-400 px-5 py-2.5 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300"
				>Register</button
			>
		</div>
	</form>
</dialog>

<dialog
	bind:this={eraseDialog}
	class="m-auto w-[calc(100%-2rem)] max-w-md rounded-lg border border-zinc-200 bg-white text-black shadow-xl backdrop:bg-zinc-950/60 backdrop:backdrop-blur-sm dark:border-zinc-800 dark:bg-zinc-900 dark:text-white"
	aria-labelledby="erase-title"
>
	<form method="dialog" class="space-y-4 p-6">
		<div class="space-y-1">
			<h2 id="erase-title" class="text-lg font-semibold">Erase this board?</h2>
			<p class="text-xs text-zinc-500">
				It did not answer as a {project?.name ?? ''} device with an id, so installing means a first flash:
				everything on it is erased, its WiFi and other settings included, then {tag} is written.
			</p>
		</div>
		<div class="flex justify-end gap-3">
			<button
				value=""
				class="cursor-pointer rounded-lg px-5 py-2.5 text-sm font-semibold text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
				>Cancel</button
			>
			<button
				value="erase"
				class="cursor-pointer rounded-lg bg-yellow-400 px-5 py-2.5 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300"
				>Erase and flash</button
			>
		</div>
	</form>
</dialog>
