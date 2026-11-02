<script lang="ts">
	// The install page's admin sign-in, mounted inside a ClerkProvider only once asked for, so an
	// anonymous visitor never loads Clerk's scripts on the page that drives the board.
	import { Show, SignIn, useClerkContext } from 'svelte-clerk/client';
	import { api } from '#lib/api.ts';
	import type { Project, Release } from '#lib/types.ts';

	let {
		slug,
		onadmin
	}: { slug: string; onadmin: (admin: { project: Project; releases: Release[] }) => void } =
		$props();

	const clerk = useClerkContext();
	let error = $state('');
	let asked = false;

	$effect(() => {
		if (!clerk.isLoaded || !clerk.session || asked) return;
		asked = true;
		(async () => {
			const found = await api.project(slug);
			if (!found) throw new Error();
			onadmin({ project: found, releases: await api.releases(found.id) });
		})().catch(() => (error = 'This account is not an admin of this project.'));
	});
</script>

<Show when="signed-out"><SignIn routing="hash" /></Show>
{#if error}
	<p class="text-sm font-semibold text-red-600" role="alert">{error}</p>
{/if}
