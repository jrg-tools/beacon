<script lang="ts">
	import { api } from '#lib/api.ts';
	import { date, size } from '#lib/format.ts';
	import List from './List.svelte';

	let { projectId, repo }: { projectId: number; repo: string } = $props();
	const releases = $derived(api.releases(projectId));
</script>

<h2 class="mb-4 text-sm font-semibold text-zinc-500 uppercase">Releases</h2>
{#await releases}
	<p class="text-xs text-zinc-500">Loading…</p>
{:then list}
	{#if list.length}
		<List items={list} key={(r) => r.tag}>
			{#snippet row(release)}
				<div class="space-y-1 px-4 py-3">
					<p class="flex flex-wrap items-center gap-2 font-semibold">
						<a
							href="https://github.com/{repo}/releases/tag/{encodeURIComponent(release.tag)}"
							target="_blank"
							rel="noopener noreferrer"
							class="hover:underline">{release.tag}</a
						>
						{#if release.prerelease}
							<span
								class="rounded-full border border-zinc-200 px-2 py-0.5 text-[11px] font-medium text-zinc-500 dark:border-zinc-700"
								>prerelease</span
							>
						{/if}
						<span class="ml-auto text-xs font-normal text-zinc-400">
							{date(release.published_at)}
						</span>
					</p>
					{#each release.assets as asset (asset.name)}
						<p class="flex min-w-0 items-center gap-1.5 text-xs text-zinc-500">
							<span
								class="size-1.5 shrink-0 rounded-full {asset.sha256
									? 'bg-emerald-500'
									: 'bg-zinc-300 dark:bg-zinc-600'}"
								title={asset.sha256 ? 'Cached on this server' : 'Still on GitHub'}
							></span>
							<span class="truncate font-medium text-zinc-700 dark:text-zinc-300">
								{asset.name}
							</span>
							<span class="shrink-0">· {size(asset.size)}</span>
							<span class="shrink-0">· {asset.sha256 ? 'cached' : 'on GitHub'}</span>
						</p>
					{/each}
				</div>
			{/snippet}
		</List>
	{:else}
		<p class="text-xs text-zinc-500">No releases with matching assets yet.</p>
	{/if}
{:catch error}
	<p class="text-sm font-semibold text-red-600" role="alert">{error.message}</p>
{/await}
