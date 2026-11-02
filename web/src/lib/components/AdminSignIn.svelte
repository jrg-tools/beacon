<script lang="ts">
	// The install page's admin mode, mounted inside a ClerkProvider only for an already signed-in
	// session, so a visitor never loads Clerk's scripts on the page that drives the board. It shows
	// nothing: no sign-in form, and a signed-in non-admin just gets the visitor's page.
	import { useClerkContext } from 'svelte-clerk/client';
	import { api } from '#lib/api.ts';
	import type { Project, Release } from '#lib/types.ts';

	let {
		slug,
		onadmin
	}: { slug: string; onadmin: (admin: { project: Project; releases: Release[] }) => void } =
		$props();

	const clerk = useClerkContext();
	let asked = false;

	$effect(() => {
		if (!clerk.isLoaded || !clerk.session || asked) return;
		asked = true;
		(async () => {
			const found = await api.project(slug);
			if (!found) throw new Error();
			onadmin({ project: found, releases: await api.releases(found.id) });
		})().catch(() => {});
	});
</script>
