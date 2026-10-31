<script lang="ts">
	import { resolve } from '$app/paths';
	import { ClerkProvider, Show, SignIn, UserButton } from 'svelte-clerk/client';
	import { fetchConfig } from '#lib/api.ts';
	import './layout.css';

	let { children } = $props();
	// The key comes from the server at runtime, so one build works for every Clerk instance.
	const config = fetchConfig();
	const year = new Date().getFullYear();
	// As in jrg-life: the header follows scrollY (scroll-driven CSS doesn't run in Safari or Firefox).
	let scrollY = $state(0);
</script>

<svelte:head><title>Beacon</title></svelte:head>
<svelte:window bind:scrollY />

{#snippet shell(signedIn: boolean)}
	<header
		class="fixed top-0 z-50 w-full px-4 py-3 transition-[background-color,backdrop-filter] duration-300 select-none sm:px-6 sm:py-4 {scrollY >
		8
			? 'bg-zinc-50/70 backdrop-blur-md dark:bg-zinc-950/70'
			: ''}"
	>
		<div class="mx-auto flex max-w-7xl items-center gap-2 sm:gap-3">
			<a href={resolve('/')} class="mr-auto flex min-w-0 flex-row items-center gap-2">
				<img src="{resolve('/')}/logo.webp" class="w-8 shrink-0 sm:w-9" alt="" />
				<span class="truncate text-lg font-bold sm:text-xl">Beacon</span>
			</a>
			{#if signedIn}<UserButton />{/if}
		</div>
	</header>
{/snippet}

<div class="flex min-h-screen flex-col">
	{#await config then { clerk_publishable_key }}
		<ClerkProvider publishableKey={clerk_publishable_key}>
			<Show when="signed-in">
				{@render shell(true)}
				<main class="mt-20 flex-grow sm:mt-24">{@render children()}</main>
				{#snippet fallback()}
					{@render shell(false)}
					<main class="mt-20 flex flex-grow flex-col items-center gap-4 px-6 py-8 sm:mt-24">
						<SignIn routing="hash" />
					</main>
				{/snippet}
			</Show>
		</ClerkProvider>
	{:catch error}
		{@render shell(false)}
		<main class="mt-20 flex-grow px-6 py-8 sm:mt-24">
			<p class="text-sm font-semibold text-red-600" role="alert">{error.message}</p>
		</main>
	{/await}

	<footer class="bg-zinc-100/20 select-none dark:bg-zinc-900/30">
		<div class="mx-auto w-full max-w-screen-xl px-4 py-6 sm:px-6 lg:py-8">
			<hr class="mb-6 border-zinc-200/60 lg:mb-8 dark:border-zinc-800/60" />
			<span class="text-sm text-zinc-300 dark:text-zinc-700">
				&copy; JRG Tools - Jorge 2013 - {year}
			</span>
		</div>
	</footer>
</div>
