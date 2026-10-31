<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { api, ApiError } from '#lib/api.ts';
	import DeviceList from '#lib/components/DeviceList.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import ProjectList from '#lib/components/ProjectList.svelte';
	import ReleaseList from '#lib/components/ReleaseList.svelte';
	import type { Project, ProjectInput } from '#lib/types.ts';

	let projects = $state<Project[]>([]);
	let error = $state('');

	// As jrg-life's `?edit=`: the open project lives in the URL.
	const selected = $derived(Number(page.url.searchParams.get('project')) || null);
	const project = $derived(projects.find((p) => p.id === selected));

	async function load() {
		try {
			projects = await api.projects();
			error = '';
		} catch (err) {
			error =
				err instanceof ApiError && err.status === 403
					? 'This account has no admin role for Beacon. Ask for role "admin" with scope "beacon" in Clerk.'
					: (err as Error).message;
		}
	}

	// Syncs run in the background on the server: refresh now and then to show how they went.
	onMount(() => {
		load();
		const timer = setInterval(load, 5000);
		return () => clearInterval(timer);
	});

	async function save(input: ProjectInput) {
		if (project) await api.updateProject(project.id, input);
		else {
			const { id } = await api.createProject(input);
			// The same page with a query string, as the list's Edit links.
			// eslint-disable-next-line svelte/no-navigation-without-resolve
			await goto(`${resolve('/')}?project=${id}`);
		}
		await load();
	}
</script>

<svelte:head><title>{project ? `${project.name} · ` : ''}Beacon</title></svelte:head>

<div
	class="mx-auto grid max-w-7xl gap-10 px-4 pt-4 pb-20 sm:px-6 sm:pt-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]"
>
	<section class="space-y-10">
		<div>
			<h1 class="mb-4 text-sm font-semibold text-zinc-500 uppercase">
				{project ? `Edit ${project.name}` : 'New project'}
			</h1>
			{#key project?.id}<ProjectForm {project} onsave={save} />{/key}
		</div>
		{#if project}
			<div><DeviceList projectId={project.id} onchange={load} /></div>
			<div><ReleaseList projectId={project.id} /></div>
		{/if}
	</section>

	<aside class="space-y-6 lg:col-start-2 lg:row-start-1">
		{#if error}
			<!-- jrg-life's sign-in notice. -->
			<p
				class="rounded-xl border border-yellow-500/40 bg-yellow-400/10 px-4 py-3 text-sm"
				role="alert"
			>
				{error}
			</p>
		{/if}
		<ProjectList {projects} {selected} onchange={load} onerror={(m) => (error = m)} />
	</aside>
</div>
