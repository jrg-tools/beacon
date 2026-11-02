<script lang="ts" module>
	export interface Line {
		at: string;
		text: string;
		kind?: 'error' | 'ok';
	}
</script>

<script lang="ts">
	// The steps and log of a USB flash (the install page's update and first flash); ConnectHint shows its progress.
	import { tick } from 'svelte';

	interface Props {
		steps: string[];
		/** The step running; `steps.length` once all are done. */
		step: number;
		running: boolean;
		error: string;
		logs: Line[];
		/** Shown once every step is done. */
		done: string;
	}
	let { steps, step, running, error, logs, done }: Props = $props();

	let terminal = $state<HTMLElement>();
	$effect(() => {
		void logs.length;
		tick().then(() => terminal?.scrollTo({ top: terminal.scrollHeight }));
	});
</script>

<section class="space-y-5" aria-live="polite">
	<ol class="grid gap-2" style="grid-template-columns: repeat({steps.length}, minmax(0, 1fr))">
		{#each steps as label, i (label)}
			{@const state = i < step ? 'done' : i === step ? (error ? 'failed' : 'active') : 'waiting'}
			<li class="space-y-2">
				<div
					class="h-1.5 overflow-hidden rounded-full {state === 'done'
						? 'bg-emerald-500'
						: state === 'failed'
							? 'bg-red-500'
							: 'bg-zinc-200 dark:bg-zinc-800'}"
				>
					{#if state === 'active'}<div
							class="sweep h-full w-1/2 rounded-full bg-yellow-400"
						></div>{/if}
				</div>
				<p
					class="flex items-center gap-1.5 text-xs font-semibold {state === 'waiting'
						? 'text-zinc-400'
						: ''}"
				>
					{#if state === 'active'}
						<span
							class="size-3 shrink-0 animate-spin rounded-full border-2 border-yellow-400 border-t-transparent"
						></span>
					{:else if state === 'done'}
						<svg
							viewBox="0 0 12 12"
							class="size-3 shrink-0 text-emerald-500"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							aria-hidden="true"><path d="M2 6.5 5 9.5 10 3" /></svg
						>
					{:else if state === 'failed'}
						<svg
							viewBox="0 0 12 12"
							class="size-3 shrink-0 text-red-500"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6" /></svg
						>
					{/if}
					<span class="min-w-0 leading-tight sm:truncate">{label}</span>
				</p>
			</li>
		{/each}
	</ol>

	{#if step >= steps.length}
		<p
			class="rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-4 py-3 text-sm"
			role="status"
		>
			{done}
		</p>
	{/if}
	{#if error}
		<p
			class="rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm font-semibold text-red-600"
			role="alert"
		>
			{error}
		</p>
	{/if}

	<div class="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-950">
		<p
			class="flex items-center gap-1.5 border-b border-zinc-800 px-4 py-2 text-[11px] font-semibold text-zinc-500 uppercase"
		>
			<span class="size-1.5 rounded-full {running ? 'animate-pulse bg-yellow-400' : 'bg-zinc-600'}"
			></span>
			Log
		</p>
		<div
			bind:this={terminal}
			class="h-72 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed text-zinc-300"
		>
			{#each logs as line, i (i)}
				<p class="break-words whitespace-pre-wrap">
					<span class="text-zinc-600">{line.at}</span>
					<span
						class={line.kind === 'error'
							? 'text-red-400'
							: line.kind === 'ok'
								? 'text-emerald-400'
								: ''}>{line.text}</span
					>
				</p>
			{/each}
			{#if running}<span class="animate-pulse text-yellow-400">▍</span>{/if}
		</div>
	</div>
</section>

<style>
	/* The active step's bar sweeps back and forth: busy, with no percentage to show. */
	.sweep {
		animation: sweep 1.2s ease-in-out infinite alternate;
	}
	@keyframes sweep {
		from {
			transform: translateX(-60%);
		}
		to {
			transform: translateX(160%);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.sweep {
			animation: none;
		}
	}
</style>
