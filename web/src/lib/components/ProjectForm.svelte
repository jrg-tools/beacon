<script lang="ts">
	import { resolve } from '$app/paths';
	import type { Project, ProjectInput } from '#lib/types.ts';

	interface Props {
		project?: Project;
		onsave: (input: ProjectInput) => Promise<void>;
	}
	let { project, onsave }: Props = $props();

	// A copy of the project as it was when opened: the page re-creates this form (`{#key}`) when
	// another project is picked, so the list's refreshes never overwrite what is being typed.
	// svelte-ignore state_referenced_locally
	let draft = $state<ProjectInput>({
		name: project?.name ?? '',
		emoji: project?.emoji ?? '📡',
		slug: project?.slug ?? '',
		repo: project?.repo ?? '',
		assets: project?.assets ?? '',
		first_flash: project?.first_flash ?? ''
	});

	let saving = $state(false);
	let error = $state('');
	let saved = $state(false);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		saving = true;
		error = '';
		saved = false;
		try {
			await onsave(draft);
			saved = true;
		} catch (err) {
			error = (err as Error).message;
		} finally {
			saving = false;
		}
	}
	// The server names the field it refused ("slug: …"), so that input is marked.
	const invalid = (field: string) => error.startsWith(field) || undefined;
</script>

<form class="space-y-3" onsubmit={submit}>
	<!-- jrg-life's emoji + name row. -->
	<div class="grid grid-cols-[4rem_minmax(0,1fr)] gap-3 sm:grid-cols-[5rem_minmax(0,1fr)]">
		<label class="space-y-1.5">
			<span class="text-sm font-semibold">Emoji</span>
			<input
				bind:value={draft.emoji}
				maxlength="16"
				class="field text-center text-xl"
				aria-invalid={invalid('emoji')}
			/>
		</label>
		<label class="space-y-1.5">
			<span class="text-sm font-semibold">Name</span>
			<input bind:value={draft.name} required maxlength="100" class="field" placeholder="AB98" />
		</label>
	</div>

	<div class="grid gap-3 sm:grid-cols-2">
		<label class="space-y-1.5">
			<span class="text-sm font-semibold">Slug</span>
			<input
				bind:value={draft.slug}
				required
				class="field"
				placeholder="ab98"
				aria-invalid={invalid('slug')}
			/>
			<span class="text-xs text-zinc-500">In the device API: /api/v1/firmware/slug/latest</span>
		</label>
		<label class="space-y-1.5">
			<span class="text-sm font-semibold">GitHub repo</span>
			<input
				bind:value={draft.repo}
				required
				class="field"
				placeholder="jrg-tools/AB98"
				aria-invalid={invalid('repo')}
			/>
			<span class="text-xs text-zinc-500">owner/name; private repos use the server's token</span>
		</label>
	</div>

	<label class="block space-y-1.5">
		<span class="text-sm font-semibold">Assets</span>
		<input
			bind:value={draft.assets}
			required
			class="field"
			placeholder="firmware.bin, firmware-filesystem.bin"
			aria-invalid={invalid('assets')}
		/>
		<span class="text-xs text-zinc-500">
			Release files devices download, comma-separated. Only semver tags with one of them count.
		</span>
	</label>

	<label class="block space-y-1.5">
		<span class="text-sm font-semibold">First flash only</span>
		<input
			bind:value={draft.first_flash}
			class="field"
			placeholder="bootloader.bin@0x0, partitions.bin@0x8000"
			aria-invalid={invalid('first_flash')}
		/>
		<span class="text-xs text-zinc-500">
			Release files a blank board needs, as name@offset (hex): written by the first flash, never
			sent as an update. The partition table among them places the assets.
		</span>
	</label>

	{#if saved}
		<p class="text-sm font-semibold text-emerald-600" role="status">
			Saved: syncing releases from GitHub.
		</p>
	{/if}
	{#if error}<p class="text-sm font-semibold text-red-600" role="alert">{error}</p>{/if}

	<div class="flex gap-3">
		<button
			disabled={saving}
			class="flex-1 cursor-pointer rounded-lg bg-yellow-400 px-6 py-2.5 font-semibold text-zinc-950 transition hover:bg-yellow-300 disabled:opacity-60 sm:flex-none"
			>{project ? 'Save' : 'Add project'}</button
		>
		{#if project}
			<a
				href={resolve('/admin')}
				class="rounded-lg px-5 py-2.5 font-semibold text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
				>Cancel</a
			>
		{/if}
	</div>
</form>
