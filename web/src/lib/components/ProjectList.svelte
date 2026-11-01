<script lang="ts">
	import { resolve } from '$app/paths';
	import { api } from '#lib/api.ts';
	import { ago } from '#lib/format.ts';
	import type { Project } from '#lib/types.ts';
	import List from './List.svelte';

	interface Props {
		projects: Project[];
		selected: number | null;
		onchange: () => void;
		onerror: (message: string) => void;
	}
	let { projects, selected, onchange, onerror }: Props = $props();

	async function run(action: () => Promise<unknown>) {
		try {
			await action();
			onchange();
		} catch (err) {
			onerror((err as Error).message);
		}
	}
</script>

<h2 class="text-sm font-semibold text-zinc-500 uppercase">Projects · {projects.length}</h2>
{#if projects.length}
	<List items={projects} key={(p) => p.id}>
		{#snippet row(project)}
			<div
				class="flex flex-wrap items-center gap-x-3 gap-y-2 py-3 pr-4 pl-4 {selected === project.id
					? 'bg-yellow-400/10'
					: ''}"
			>
				<span
					class="grid size-10 shrink-0 place-items-center rounded-lg bg-amber-100 text-xl dark:bg-amber-500/15"
					aria-hidden="true">{project.emoji}</span
				>
				<div class="min-w-0 flex-1">
					<p class="flex items-center gap-2 truncate font-semibold">
						{project.name}
						<span
							class="rounded-full border border-zinc-200 px-2 py-0.5 text-[11px] font-medium text-zinc-500 dark:border-zinc-700"
							>{project.latest ?? 'no release'}</span
						>
					</p>
					<p class="truncate text-xs text-zinc-400">
						{project.repo} · {project.devices} devices · synced {ago(project.synced_at)}
					</p>
					{#if project.sync_error}
						<p class="truncate text-xs text-red-600" title={project.sync_error}>
							Sync failed: {project.sync_error}
						</p>
					{/if}
				</div>
				<div class="-mx-3 flex w-full justify-end sm:mx-0 sm:w-auto">
					<a href={resolve('/admin/project/[slug]', { slug: project.slug })} class="action">Open</a>
					<a href="{resolve('/admin')}?project={project.id}" class="action">Edit</a>
					<button class="action" onclick={() => run(() => api.syncProject(project.id))}>
						Sync
					</button>
					<button
						class="action action-danger"
						onclick={() => {
							if (confirm(`Delete ${project.name}, its devices and its cached firmware?`))
								run(() => api.deleteProject(project.id));
						}}>Delete</button
					>
				</div>
			</div>
		{/snippet}
	</List>
{:else}
	<p class="text-xs text-zinc-500">
		No projects yet: add a GitHub repo whose releases carry firmware.
	</p>
{/if}
