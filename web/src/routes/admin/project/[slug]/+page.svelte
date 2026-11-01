<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { api } from '#lib/api.ts';
	import DeviceList from '#lib/components/DeviceList.svelte';
	import ProjectStats from '#lib/components/ProjectStats.svelte';
	import ReleaseList from '#lib/components/ReleaseList.svelte';
	import { ago } from '#lib/format.ts';
	import type { Project, ProjectStats as Stats } from '#lib/types.ts';

	const slug = $derived(page.params.slug ?? '');

	let project = $state<Project | null>(null);
	// A number, so the lists below only reload when another project is opened.
	const id = $derived(project?.id ?? 0);
	let stats = $state<Stats | null>(null);
	let error = $state('');

	async function load() {
		try {
			const found = (await api.projects()).find((p) => p.slug === slug) ?? null;
			stats = found ? await api.stats(found.id) : null;
			project = found;
			error = project ? '' : 'No such project.';
		} catch (err) {
			error = (err as Error).message;
		}
	}

	// Refreshed in place in the background: nothing below is re-created.
	onMount(() => {
		load();
		const timer = setInterval(load, 30000);
		return () => clearInterval(timer);
	});
</script>

<svelte:head><title>{project ? `${project.name} · ` : ''}Beacon</title></svelte:head>

<div class="mx-auto max-w-7xl space-y-10 px-4 pb-20 sm:px-6">
	<!-- The back link sits right above the name, not a whole section gap away. -->
	<div class="space-y-3">
		<a
			href={resolve('/admin')}
			class="-ml-3 inline-block rounded-lg px-3 py-1.5 text-sm font-semibold text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800/60"
			>← Projects</a
		>

		{#if error}
			<p
				class="rounded-lg border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm"
				role="alert"
			>
				{error}
			</p>
		{/if}

		{#if project}
			<header class="flex flex-wrap items-center gap-4">
				<span
					class="grid size-14 shrink-0 place-items-center rounded-lg bg-amber-100 text-3xl dark:bg-amber-500/15"
					aria-hidden="true">{project.emoji}</span
				>
				<div class="min-w-0 flex-1">
					<h1 class="truncate text-xl font-semibold">{project.name}</h1>
					<p class="truncate text-xs text-zinc-400">
						<a
							href="https://github.com/{project.repo}"
							target="_blank"
							rel="noopener noreferrer"
							class="font-medium text-zinc-500 transition hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100"
							>{project.repo} ↗</a
						>
						· {project.latest ?? 'no release'} · synced {ago(project.synced_at)}
					</p>
				</div>
				<!-- Its own row on phones, so the name keeps the width. -->
				<nav class="-ml-3 flex w-full gap-1 sm:ml-0 sm:w-auto">
					<a href="{resolve('/admin')}?project={project.id}" class="action">Edit</a>
					<a href={resolve('/firmware/[slug]', { slug: project.slug })} class="action">Install</a>
					<a href={resolve('/admin/project/[slug]/flash', { slug: project.slug })} class="action"
						>First flash</a
					>
				</nav>
			</header>
		{/if}
	</div>

	{#if project}
		{#if stats}
			<section>
				<h2 class="mb-4 text-sm font-semibold text-zinc-500 uppercase">Statistics · 30 days</h2>
				<ProjectStats {stats} />
			</section>
		{/if}

		<div class="grid grid-cols-1 gap-10 lg:grid-cols-2">
			<section><DeviceList projectId={id} onchange={load} /></section>
			<!-- Reloads only when a sync finishes. -->
			{#key project.synced_at}
				<section><ReleaseList projectId={id} repo={project.repo} /></section>
			{/key}
		</div>
	{/if}
</div>
