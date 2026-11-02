<script lang="ts">
	import { beforeNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import { getContext, onMount } from 'svelte';
	import { ClerkProvider } from 'svelte-clerk/client';
	import { ApiError, api, deviceGet, fetchConfig, projectReleases, register } from '#lib/api.ts';
	import AdminSignIn from '#lib/components/AdminSignIn.svelte';
	import ConnectHint from '#lib/components/ConnectHint.svelte';
	import FlashProgress, { type Line } from '#lib/components/FlashProgress.svelte';
	import {
		type Board,
		backup,
		bootPrevious,
		chipName,
		extras,
		firstFlashFiles,
		flash,
		probe,
		restore
	} from '#lib/flash.ts';
	import { date, size } from '#lib/format.ts';
	import { type SerialPort, hasSerial, pick, reconnect } from '#lib/serial.ts';
	import type { InstallProject, Project, Release } from '#lib/types.ts';

	const slug = $derived(page.params.slug ?? '');

	let project = $state<InstallProject | null>(null);
	// A 404: a private project (or none) for anyone but its admin. The board's signed `latest` then
	// says which release it gets, so nothing about the project shows before the board proves itself.
	let hidden = $state(false);
	// Clerk for an already signed-in admin, loaded only then: its key.
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
	// A forced full flash; without it, a public project still detects a board that needs one.
	let erase = $state(false);
	// A first flash that keeps NVS (WiFi, Beacon key and id): a repair of the bootloader and table.
	let keep = $state(false);
	// An admin's may erase; a public project's visitor only repairs (keeps settings and id).
	const canFull = $derived(flashExtras.length > 0 && (!!admin || !!project?.public));
	const keeps = $derived(keep || !admin);

	const STEPS = ['Inspect device', 'Verify device', 'Download', 'Flash', 'Restart', 'Check boot'];
	const FIRST_STEPS = [
		'Inspect device',
		'Download',
		'Erase and flash',
		'Restart',
		'Check boot',
		'Register'
	];
	let steps = $state(STEPS);
	// Which step writes (or reads) flash: ConnectHint's progress bar; -1 for none.
	let flashStep = $state(3);
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
	// A recovery tool's captions for ConnectHint; none for an install.
	let captions = $state<[string, string][]>();
	// ConnectHint following the run: the port popup, verifying, installing, done.
	const live = $derived(
		step < 0
			? null
			: {
					scene: picking ? 1 : step >= steps.length ? 4 : step === 0 && !captions ? 2 : 3,
					progress,
					failed: !!error && !running,
					tag: installing,
					captions
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
		// Coming from the admin pages, Clerk is already loaded and signed in; on a reload, its
		// `__client_uat` cookie (0 when signed out) says someone is, so a visitor never loads Clerk.
		if (window.Clerk?.session || /(?:^|; )__client_uat[^=]*=[1-9]/.test(document.cookie))
			signIn = fetchConfig();
	});

	// The layout's header, which links to the admin pages for an admin.
	const shell = getContext<{ admin: boolean }>('install');

	// Signed in as this project's admin: the full page, the private project's included.
	async function onadmin(found: NonNullable<typeof admin>) {
		admin = found;
		shell.admin = true;
		if (!project) await load(); // now with the admin's session
	}

	function log(text: string, kind?: Line['kind']) {
		for (const line of text.split('\n').filter((l) => l.trim()))
			logs.push({ at: new Date().toLocaleTimeString(), text: line, kind });
	}

	type Firmware = Awaited<ReturnType<typeof reconnect>>;
	// The open session with the board's firmware, closed when a run ends however it ends.
	let board: Firmware | undefined;

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
		if (fraction >= 1) step = flashStep + 1;
	};

	type Target = {
		tag: string;
		assets: { name: string; size: number; chip?: number | null }[];
	};

	// The release whose app has this ELF hash (`app_sha`, read by the server from each cached
	// release), so the page names what a board runs without asking its firmware.
	const tagOf = (sha: string) =>
		(admin?.releases ?? project?.releases ?? []).find((r) =>
			r.assets.some((a) => a.app_sha === sha)
		)?.tag;

	// What the bootloader found, in the log: chip, table and what each app slot holds.
	function describe(seen: Board) {
		log(
			`${seen.chip}, ${seen.table.length ? `${seen.table.length} partitions` : 'no partition table (blank)'}.`
		);
		for (const p of seen.table)
			log(`Partition ${p.label} at 0x${p.offset.toString(16)} (${size(p.size)})`);
		for (const { partition, chip, desc } of seen.apps) {
			const what =
				chip === undefined
					? 'empty'
					: desc
						? (tagOf(desc.sha) ?? `${desc.project} ${desc.version}, built ${desc.built}`)
						: 'an app';
			log(`${partition.label}: ${what}${partition === seen.running ? ' (boots)' : ''}`);
		}
	}

	// Before anything is downloaded: the chip the release's images are built for (the server reads
	// it from each cached file; unknown for one not cached yet, which the flash checks instead).
	function expectChip(seen: Board, rel: Target) {
		const want = rel.assets.find((a) => a.chip != null)?.chip;
		if (want != null && seen.chipId !== undefined && want !== seen.chipId)
			throw new Error(
				`Connected device is an ${seen.chip}, but ${project?.name ?? 'this firmware'} ${rel.tag} is built for the ${chipName(want)}. Wrong device? Nothing was downloaded or written.`
			);
	}

	// Verified before anything is written, and again once the new firmware has booted. NVS stays.
	// Without `rel` (a private project's page for a non-admin), the release is whatever the board's
	// own `latest` offers it.
	async function update(
		port: SerialPort,
		rel: Target | undefined,
		before: { id: string; version: string },
		seen: Board
	) {
		step = 1;
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
			expectChip(seen, rel);
		}
		installing = rel.tag;

		// While the board is still connected: it signs each download.
		step = 2;
		const assets = [];
		for (const asset of rel.assets) {
			if (flashExtras.some((e) => e.name === asset.name)) continue;
			log(`Downloading ${asset.name} (${size(asset.size)})…`);
			const res = await boardGet(before.id, assetPath(rel.tag, asset.name));
			assets.push({ name: asset.name, data: new Uint8Array(await res.arrayBuffer()) });
		}
		await board!.close();
		board = undefined;

		step = 3;
		log(`Checking the images, then entering the bootloader to install ${rel.tag}…`);
		await flash(port, { assets }, (line) => log(line), flashing);
		step = 5;
		log('Waiting for the device to boot…');
		board = await reconnect(port);
		const after = await info(
			30000,
			'The device came back on USB but did not answer: the new firmware may not boot. Boot the previous app from the Advanced menu, or check its serial log.'
		);
		if (after.id !== before.id)
			throw new Error(`A different device answered after the restart (${after.id}).`);
		if (!sameVersion(after.version, rel.tag))
			throw new Error(`It booted ${after.version}, not ${rel.tag}.`);
		await check(after.id, after.version);
		log(`Your device booted ${after.version}, key intact.`, 'ok');
		done = `Your device is on ${rel.tag}.`;
	}

	// A blank or bricked board: everything is written, bootloader and partition table included, the
	// flash erased first unless `keepNvs` (a repair, keeping WiFi, key and id). Then it gets its id:
	// issued by Beacon (public) or registered with a key (private), unless it kept one.
	// `known`: the id the board already has, if it said: a public one keeps it, no new one made.
	async function firstFlash(
		port: SerialPort,
		rel: NonNullable<typeof release>,
		keepNvs: boolean,
		known = ''
	) {
		steps = FIRST_STEPS;
		flashStep = 2;
		await board?.close();
		board = undefined;

		step = 1;
		let id = '';
		if (project?.public) {
			// Its first check, made for it: the id carries the downloads and is written once it boots.
			// One Beacon refuses (unknown, or another project's) gets a new one.
			const path = `/api/v1/firmware/${slug}/latest`;
			const res = await deviceGet(path, known ? { device: known } : {}).catch((err) => {
				if (known && (err.status === 401 || err.status === 403)) return deviceGet(path, {});
				throw err;
			});
			id = (await res.json()).device;
			log(id === known ? `Keeping its id ${id}.` : `Beacon gave the device the id ${id}.`);
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
		log(
			keepNvs
				? `Rewriting the bootloader, table and ${rel.tag}, settings kept…`
				: `Erasing the board and writing ${rel.tag}…`
		);
		await flash(port, { files }, (line) => log(line), flashing, !keepNvs);
		step = 4;
		log('Waiting for the device to boot…');
		board = await reconnect(port);
		const after = await info(
			30000,
			'The device came back on USB but did not answer: the firmware may not boot. Check its serial log.'
		);
		log(`The device booted ${after.version}.`, 'ok');

		step = 5;
		// A public board whose old id Beacon refused is given the new one.
		if (keepNvs && after.id !== 'none' && !(known && known !== id)) {
			id = after.id;
			log(`It kept its id ${id}.`);
		} else if (project?.public)
			await board.ask(`beacon id ${id}`, `beacon:ok ${id}`).catch(() => {
				throw new Error(`The device did not save its id ${id}: flash it again.`);
			});
		else {
			const name = await askName();
			if (!name)
				throw new Error(
					`The device runs ${after.version} but is not registered: sync it on the project page.`
				);
			id = await register(board, admin!.project.id, name);
		}
		await check(id, after.version);
		log(`Your device is ${id}.`, 'ok');
		done = `Your device runs ${after.version} as ${id}${project?.public || keepNvs ? '' : ', its key made on the board'}.`;
	}

	// Runs one USB job with its own steps and log: picks the port first, while the click still
	// allows the port picker, and closes whatever is open however it ends.
	async function run(
		list: string[],
		writes: number,
		work: (port: SerialPort) => Promise<void>,
		lines?: [string, string][]
	) {
		captions = lines;
		error = '';
		logs = [];
		done = '';
		progress = 0;
		steps = list;
		flashStep = writes;
		running = true;
		installing = '';
		step = 0;
		try {
			picking = true;
			const port = await pick();
			picking = false;
			await work(port);
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

	// The bootloader first, whatever the board runs: its chip, table and installed app decide the
	// rest. Its firmware is asked only who the board is (and, on a private project, to sign).
	function install() {
		if (!hidden && (!project || !release)) return;
		return run(STEPS, 3, async (port) => {
			log('Entering the bootloader to see what the board holds…');
			const seen = await probe(port, (line) => log(line));
			describe(seen);
			if (release) expectChip(seen, release);
			if (canFull && erase) {
				let known = '';
				if (project?.public && keeps) {
					log('Asking its firmware who it is, to keep its id…');
					board = await reconnect(port);
					const { id } = await info(10000, '').catch(() => ({ id: 'none' }));
					if (id !== 'none') known = id;
				}
				return firstFlash(port, release!, keeps, known);
			}
			if (!seen.table.length) {
				if (!project?.public)
					throw new Error(
						'The board is blank (no partition table): it needs a first flash, which its admin does after Admin sign-in.'
					);
				return confirmFirstFlash(port, seen, 'The board is blank: first flash.');
			}
			log('Asking its firmware who it is…');
			board = await reconnect(port);
			// A public project's board that doesn't answer runs no (or foreign) firmware.
			const before = await info(
				project?.public ? 10000 : 15000,
				'The device did not answer "beacon info": its firmware predates installs from here, so update it over the air or with PlatformIO once.'
			).catch((err) => {
				if (project?.public) return { id: 'none', version: '' };
				throw err;
			});
			if (project?.public && before.id === 'none')
				return confirmFirstFlash(
					port,
					seen,
					before.version
						? 'The device has no id yet: first flash.'
						: 'No answer to "beacon info" (other firmware): first flash.'
				);
			return update(port, release, before, seen);
		});
	}

	// Anyone's board may be plugged in: never wipe it without asking. One with a partition table can
	// be repaired instead, keeping its settings.
	async function confirmFirstFlash(port: SerialPort, seen: Board, why: string) {
		log(why);
		keepable = seen.table.length > 0;
		const answer = await ask(eraseDialog!);
		if (!answer) throw new Error('Cancelled: the board was not touched.');
		await firstFlash(port, release!, answer === 'keep');
	}

	// Recovery tools: the bootloader only, so they work whatever the board runs, or if it runs nothing.

	function saveBackup() {
		return run(
			['Read flash', 'Save'],
			0,
			async (port) => {
				const { image, chip } = await backup(port, (line) => log(line), flashing);
				const name = `${slug}-${chip.toLowerCase()}-${new Date().toISOString().slice(0, 10)}.bin`;
				const url = URL.createObjectURL(new Blob([image]));
				Object.assign(document.createElement('a'), { href: url, download: name }).click();
				URL.revokeObjectURL(url);
				done = `Saved ${name} (${size(image.length)}). It holds everything on the board, its WiFi password and Beacon key included: keep it private.`;
			},
			[
				['Backing up', 'Reading the whole flash into a file. Keep it plugged in.'],
				['Backed up', 'The file is saved: keep it private, it holds the board’s secrets.']
			]
		);
	}

	// The backup `restoreBackup` writes, picked first: a file picker's choice is not a click the port
	// picker accepts.
	let backupFile = $state<File | null>(null);
	let toolsDialog = $state<HTMLDialogElement>();

	function restoreBackup() {
		const file = backupFile;
		if (!file) return;
		return run(
			['Write backup', 'Restart'],
			0,
			async (port) => {
				if (
					!confirm(
						`Write ${file.name} over everything on this board? Its current contents are lost.`
					)
				)
					throw new Error('Cancelled: the board was not touched.');
				await restore(
					port,
					new Uint8Array(await file.arrayBuffer()),
					(line) => log(line),
					flashing
				);
				done = `The board was restored from ${file.name}.`;
			},
			[
				['Restoring', 'Writing the backup over the whole flash. Keep it plugged in.'],
				['Restored', 'The board runs what the backup held, settings included.']
			]
		);
	}

	function bootPreviousApp() {
		return run(
			['Switch app', 'Restart'],
			-1,
			async (port) => {
				const slot = await bootPrevious(port, (line) => log(line));
				step = 1;
				const what = slot.desc ? (tagOf(slot.desc.sha) ?? slot.desc.version) : 'its app';
				done = `The board now boots ${slot.partition.label}: ${what}.`;
			},
			[
				['Switching app', 'Pointing the bootloader at the other app slot.'],
				['Switched', 'The board now boots the app the last install replaced.']
			]
		);
	}

	// A first flash's last step on a private project: Sync device's name, asked in the page.
	let dialog = $state<HTMLDialogElement>();
	let deviceName = $state('');
	// A public project's first flash erases the board: confirmed first.
	let eraseDialog = $state<HTMLDialogElement>();
	// The board has a partition table, so the dialog also offers a repair that keeps its settings.
	let keepable = $state(false);

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
	{#if !hasSerial()}
		<p class="rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm">
			Installing talks to the device over USB, which needs Chrome or Edge on a computer.
		</p>
	{/if}
	{#if error && step < 0}
		<p class="text-sm font-semibold text-red-600" role="alert">{error}</p>
	{/if}
	<!-- The board signs for a private project, an admin's update included; a public one's just answers. -->
	<div class="relative">
		<ConnectHint {live} signed={project?.public !== true} />
		<button
			onclick={() => toolsDialog?.showModal()}
			class="absolute top-3 right-3 cursor-pointer rounded-lg p-2 text-zinc-400 transition hover:bg-zinc-200/60 hover:text-zinc-700 dark:hover:bg-zinc-800/60 dark:hover:text-zinc-200"
			title="Advanced"
			aria-label="Advanced"
		>
			<svg
				viewBox="0 0 24 24"
				class="size-5"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
				aria-hidden="true"
				><path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" /><circle
					cx="15"
					cy="6"
					r="2"
				/><circle cx="9" cy="12" r="2" /><circle cx="17" cy="18" r="2" /></svg
			>
			<!-- Full flash is on: the drawer holds a setting the install button uses. -->
			{#if canFull && erase}
				<span class="absolute top-1.5 right-1.5 size-2 rounded-full bg-yellow-400"></span>
			{/if}
		</button>
	</div>

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
		{/if}
		<button
			onclick={install}
			disabled={running || (!hidden && !release) || !hasSerial()}
			class="w-full cursor-pointer rounded-lg bg-yellow-400 px-5 py-3 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300 disabled:cursor-not-allowed disabled:opacity-50"
			>{running
				? 'Installing…'
				: hidden
					? 'Connect and update'
					: canFull && erase
						? `Connect, ${keeps ? 'repair' : 'erase'} and flash ${tag}`
						: `Connect and install ${tag}`}</button
		>
		{#if !admin && signIn}
			{#await signIn then { clerk_publishable_key }}
				<ClerkProvider publishableKey={clerk_publishable_key}>
					<AdminSignIn {slug} {onadmin} />
				</ClerkProvider>
			{:catch}
				<!-- Admin mode only: a visitor's page stays as it is. -->
			{/await}
		{/if}
	</section>

	{#if step >= 0}
		<FlashProgress {steps} {step} {running} {error} {logs} {done} />
	{/if}
</div>

<!-- Advanced: a drawer from the right. A click on the backdrop (the dialog itself) closes it;
	a tool closes it too, so the run's progress shows on the page. -->
<dialog
	bind:this={toolsDialog}
	onclick={(e) => e.target === toolsDialog && toolsDialog.close()}
	class="drawer m-0 ml-auto h-dvh max-h-none w-full max-w-sm border-l border-zinc-200 bg-white text-black shadow-xl backdrop:bg-zinc-950/40 backdrop:backdrop-blur-sm dark:border-zinc-800 dark:bg-zinc-900 dark:text-white"
	aria-labelledby="tools-title"
>
	<div class="flex h-full flex-col gap-6 p-6">
		<div class="flex items-start justify-between gap-3">
			<h2 id="tools-title" class="text-lg font-semibold">Advanced</h2>
			<button
				onclick={() => toolsDialog?.close()}
				class="-m-1 cursor-pointer rounded-lg p-1 text-zinc-400 hover:bg-zinc-200/60 hover:text-zinc-700 dark:hover:bg-zinc-800/60 dark:hover:text-zinc-200"
				aria-label="Close"
				><svg
					viewBox="0 0 24 24"
					class="size-5"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg
				></button
			>
		</div>
		{#if canFull}
			<section class="space-y-1">
				<h3 class="text-xs font-semibold text-zinc-500 uppercase">Installation</h3>
				<label class="flex cursor-pointer items-start justify-between gap-4 py-3">
					<span class="space-y-0.5">
						<span class="block text-sm font-semibold">Full flash</span>
						<span class="block text-xs text-zinc-500"
							>Writes the bootloader and partition table too, not just the app. {admin
								? 'Erases everything unless Keep settings is on, then registers the board.'
								: 'Its settings and id stay.'} For a blank, bricked or broken board.</span
						>
					</span>
					<input
						type="checkbox"
						role="switch"
						bind:checked={erase}
						disabled={running}
						class="switch"
					/>
				</label>
				{#if erase && admin}
					<label class="flex cursor-pointer items-start justify-between gap-4 py-3 pl-4">
						<span class="space-y-0.5">
							<span class="block text-sm font-semibold">Keep settings</span>
							<span class="block text-xs text-zinc-500"
								>A repair: no erase, so WiFi, key and id stay.</span
							>
						</span>
						<input
							type="checkbox"
							role="switch"
							bind:checked={keep}
							disabled={running}
							class="switch"
						/>
					</label>
				{/if}
			</section>
		{/if}
		{#if release && !hidden}
			<section class="space-y-3">
				<h3 class="text-xs font-semibold text-zinc-500 uppercase">Files in {tag}</h3>
				<ul class="grid gap-x-4 gap-y-2">
					{#each release.assets as asset (asset.name)}
						{@const extra = flashExtras.find((e) => e.name === asset.name)}
						<li class="flex min-w-0 items-start gap-2 text-xs">
							<span class="mt-1.5 size-1.5 shrink-0 rounded-full bg-zinc-300 dark:bg-zinc-600"
							></span>
							<span class="min-w-0">
								<span class="block truncate font-medium text-zinc-700 dark:text-zinc-300"
									>{asset.name}</span
								>
								<span class="block text-zinc-500"
									>{size(asset.size)}{extra
										? ` · full flash at 0x${extra.address.toString(16)}`
										: ''}</span
								>
							</span>
						</li>
					{/each}
				</ul>
			</section>
		{/if}
		<section class="space-y-1">
			<h3 class="text-xs font-semibold text-zinc-500 uppercase">Recovery tools</h3>
			<p class="text-xs text-zinc-500">
				These talk to the board's bootloader, so they work whatever it runs, or if it runs nothing.
			</p>
			<div class="divide-y divide-zinc-200 dark:divide-zinc-800">
				{#each [{ label: 'Back up the whole flash', hint: 'Reads everything into a file: minutes for 16 MB.', action: saveBackup }, { label: 'Boot the previous app', hint: 'Boots the app the last install replaced, still in the other slot.', action: bootPreviousApp }] as tool (tool.label)}
					<button
						onclick={() => {
							toolsDialog?.close();
							tool.action();
						}}
						disabled={running || !hasSerial()}
						class="-mx-2 block w-[calc(100%+1rem)] cursor-pointer rounded-lg px-2 py-3 text-left transition hover:bg-zinc-200/60 disabled:cursor-not-allowed disabled:opacity-50 dark:hover:bg-zinc-800/60"
					>
						<span class="block text-sm font-semibold">{tool.label}</span>
						<span class="block text-xs text-zinc-500">{tool.hint}</span>
					</button>
				{/each}
				<div class="space-y-3 py-3">
					<div>
						<span class="block text-sm font-semibold">Restore a backup</span>
						<span class="block text-xs text-zinc-500"
							>Writes it back whole, once its size, bootloader and partition table check out.</span
						>
					</div>
					<input
						type="file"
						accept=".bin,application/octet-stream"
						disabled={running}
						onchange={(e) => (backupFile = e.currentTarget.files?.[0] ?? null)}
						class="w-full text-xs text-zinc-500 file:mr-3 file:cursor-pointer file:rounded-lg file:border-0 file:bg-zinc-200 file:px-3 file:py-2 file:text-xs file:font-semibold dark:file:bg-zinc-800"
						aria-label="Backup file to restore"
					/>
					<button
						onclick={() => {
							toolsDialog?.close();
							restoreBackup();
						}}
						disabled={running || !backupFile || !hasSerial()}
						class="w-full cursor-pointer rounded-lg border border-zinc-200 px-4 py-2.5 text-sm font-semibold transition hover:bg-zinc-200/60 disabled:cursor-not-allowed disabled:opacity-50 dark:border-zinc-800 dark:hover:bg-zinc-800/60"
						>Restore backup</button
					>
				</div>
			</div>
		</section>
		<p class="mt-auto text-xs text-zinc-500">
			A backup holds everything on the board, its WiFi password and Beacon key included: keep it
			private.
		</p>
	</div>
</dialog>

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
			{#if keepable}
				<p class="text-xs text-zinc-500">
					Or repair it: the bootloader, partition table and {tag} are rewritten, settings kept.
				</p>
			{/if}
		</div>
		<div class="flex justify-end gap-3">
			<button
				value=""
				class="cursor-pointer rounded-lg px-5 py-2.5 text-sm font-semibold text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
				>Cancel</button
			>
			{#if keepable}
				<button
					value="keep"
					class="cursor-pointer rounded-lg px-5 py-2.5 text-sm font-semibold hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
					>Repair</button
				>
			{/if}
			<button
				value="erase"
				class="cursor-pointer rounded-lg bg-yellow-400 px-5 py-2.5 text-sm font-semibold text-zinc-950 transition hover:bg-yellow-300"
				>Erase and flash</button
			>
		</div>
	</form>
</dialog>

<style>
	@media (prefers-reduced-motion: no-preference) {
		/* Transitions, not an animation, so closing slides out too: `display` and `overlay` stay
		   discrete until the slide ends, keeping the closed dialog in the top layer meanwhile. */
		.drawer,
		.drawer::backdrop {
			transition:
				transform 0.2s ease-out,
				opacity 0.2s ease-out,
				display 0.2s allow-discrete,
				overlay 0.2s allow-discrete;
		}
		.drawer:not([open]) {
			transform: translateX(100%);
		}
		.drawer:not([open])::backdrop {
			opacity: 0;
		}
		@starting-style {
			.drawer[open] {
				transform: translateX(100%);
			}
			.drawer[open]::backdrop {
				opacity: 0;
			}
		}
	}
</style>
