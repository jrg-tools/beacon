<!--
	The install page of a private project (or an unknown slug: the 404 is the same) for anyone but its
	admin. The registered board is the key, so this walks through using it: plug it in, pick it in the
	browser's popup, it proves itself, it updates. Idle, it plays the walkthrough on its own (a step
	click jumps to it); during a run, `live` drives it from the real steps.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { fade, fly, scale } from 'svelte/transition';

	/** 0 plug in (the demo only), 1 port popup, 2 proving, 3 updating, 4 done. */
	let {
		live = null
	}: { live?: { scene: number; progress: number; failed: boolean; tag?: string } | null } =
		$props();

	const STEPS = [
		['Plug it in', 'Your device, over USB, on this computer.'],
		['Pick it', 'Choose it in the browser’s popup.'],
		['It proves itself', 'It signs with its own key, which never leaves it.'],
		['It updates', 'To its newest release, and only it gets it.']
	];
	const SCENE_MS = 3000;

	let demo = $state(0);
	// A step clicked by hand holds the demo there for a while.
	let heldUntil = 0;
	let reduced = $state(false);
	const scene = $derived(live?.scene ?? demo);
	const failed = $derived(live?.failed ?? false);
	// The demo fills the bar in its scene; a run shows the real flash.
	let demoFill = $state(0);
	const fill = $derived(live ? live.progress : demoFill);

	onMount(() => {
		reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
		if (reduced) demo = 1; // the popup: the one scene worth a still
		const timer = setInterval(() => {
			if (live || reduced || Date.now() < heldUntil) return;
			demo = (demo + 1) % 5;
		}, SCENE_MS);
		return () => clearInterval(timer);
	});

	// Starts empty, then fills over the scene once the browser has painted the empty bar.
	$effect(() => {
		if (scene !== 3 || live) return;
		demoFill = 0;
		const t = setTimeout(() => (demoFill = 1), 80);
		return () => clearTimeout(t);
	});

	function pick(i: number) {
		if (live) return;
		demo = i;
		heldUntil = Date.now() + SCENE_MS * 2;
	}

	// The step each scene belongs to; done counts as past the last.
	const stepOf = (s: number) => Math.min(s, 3) + (s === 4 ? 1 : 0);

	const CAPTIONS = [
		['This firmware is private', 'It opens with a registered device, not a password.'],
		['Pick your device', 'The browser asks which USB port to use: choose your device.'],
		['Your device is proving itself', 'It signs the request with its own key.'],
		['Installing', 'Downloading and writing its newest release. Keep it plugged in.'],
		['Up to date', 'Your device runs its newest release, settings and key intact.']
	];
	const caption = $derived(
		failed ? ['That didn’t work', 'See the message below, then try again.'] : CAPTIONS[scene]
	);
</script>

<div
	class="relative overflow-hidden rounded-lg border border-zinc-200 bg-white p-5 sm:p-6 dark:border-zinc-800 dark:bg-zinc-900"
	class:motion={!reduced}
>
	<div
		class="glow pointer-events-none absolute inset-x-0 -top-24 mx-auto size-72 rounded-full blur-3xl transition-colors duration-700 {failed
			? 'bg-red-500/15'
			: scene === 4
				? 'bg-emerald-400/20'
				: 'bg-yellow-400/20'}"
	></div>

	<!-- The scene: a computer, the cable with the lock over it, the board. -->
	<div class="relative mx-auto flex max-w-md items-end gap-0" aria-hidden="true">
		<div class="w-[56%] shrink-0">
			<div
				class="relative aspect-[16/10] overflow-hidden rounded-md border-2 border-zinc-300 bg-zinc-100 dark:border-zinc-700 dark:bg-zinc-800"
			>
				<!-- The browser around this page. -->
				<div
					class="flex items-center gap-1 border-b border-zinc-200 px-1.5 py-1 dark:border-zinc-700"
				>
					{#each [0, 1, 2] as dot (dot)}
						<span class="size-1 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
					{/each}
					<span
						class="ml-1 truncate rounded-full bg-white px-1.5 text-[7px] leading-3 text-zinc-400 dark:bg-zinc-900"
						>beacon.jrg.tools</span
					>
				</div>
				<div class="space-y-1 p-2">
					<span class="block h-1 w-1/3 rounded-full bg-yellow-400"></span>
					<span class="block h-1 w-2/3 rounded-full bg-zinc-300 dark:bg-zinc-700"></span>
					<span class="block h-1 w-1/2 rounded-full bg-zinc-300 dark:bg-zinc-700"></span>
				</div>

				{#if scene === 1 && !failed}
					<!-- Chrome's serial port chooser, as it drops from the address bar. -->
					<div
						class="absolute inset-x-[7%] top-[14%] rounded-md bg-white p-1.5 shadow-xl ring-1 ring-zinc-200 dark:bg-zinc-900 dark:ring-zinc-700"
						in:fly={{ y: -10, duration: 350 }}
						out:fade={{ duration: 150 }}
					>
						<p class="text-[7px] leading-tight text-zinc-600 dark:text-zinc-300">
							<b>beacon.jrg.tools</b> wants to connect to a serial port
						</p>
						<div class="mt-1 space-y-0.5 text-[6.5px] leading-tight">
							<span class="row block truncate rounded px-1 py-0.5">USB JTAG/serial debug unit</span>
							<span class="block truncate rounded px-1 py-0.5 text-zinc-400"
								>Bluetooth-Incoming-Port</span
							>
						</div>
						<div class="mt-1 flex justify-end gap-1 text-[6.5px] font-semibold">
							<span
								class="rounded px-1.5 py-0.5 text-zinc-500 ring-1 ring-zinc-200 dark:ring-zinc-700"
								>Cancel</span
							>
							<span class="connect rounded bg-zinc-300 px-1.5 py-0.5 text-zinc-500 dark:bg-zinc-700"
								>Connect</span
							>
						</div>
						{#if !live}
							<!-- The pointer picking the device, then Connect. -->
							<svg viewBox="0 0 12 16" class="cursor absolute size-3">
								<path
									d="M1 1v12l3-3 2 5 2-1-2-5h4z"
									class="fill-zinc-900 stroke-white dark:fill-white dark:stroke-zinc-900"
								/>
							</svg>
						{/if}
					</div>
				{:else if scene === 2 && !failed}
					<div
						class="absolute inset-0 top-[18%] grid place-items-center"
						in:scale={{ start: 0.8, duration: 300 }}
					>
						<span class="flex items-center gap-1 text-[8px] font-semibold text-zinc-500">
							<span
								class="spin size-2.5 rounded-full border-2 border-yellow-400 border-t-transparent"
							></span>
							Verifying…
						</span>
					</div>
				{:else if scene === 3 && !failed}
					<div class="absolute inset-x-[12%] top-[52%] space-y-1" in:fade={{ duration: 250 }}>
						<span class="flex justify-between text-[8px] font-semibold text-zinc-500">
							<span>{live?.tag ? `Installing ${live.tag}` : 'Installing'}</span>
							<span>{Math.round(fill * 100)}%</span>
						</span>
						<span class="block h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-700">
							<span
								class="block h-full rounded-full bg-yellow-400 transition-[width] ease-out {live
									? 'duration-500'
									: 'duration-[2400ms]'}"
								style="width: {fill * 100}%"
							></span>
						</span>
					</div>
				{:else if scene === 4 || failed}
					<div
						class="absolute inset-0 top-[18%] grid place-items-center"
						in:scale={{ start: 0.6, duration: 350 }}
					>
						<span
							class="grid size-7 place-items-center rounded-full text-white {failed
								? 'bg-red-500'
								: 'bg-emerald-500'}"
						>
							<svg
								viewBox="0 0 16 16"
								class="size-4"
								fill="none"
								stroke="currentColor"
								stroke-width="2.5"
								stroke-linecap="round"
								stroke-linejoin="round"
							>
								<path d={failed ? 'M4.5 4.5l7 7m0-7l-7 7' : 'M3.5 8.5l3 3 6-7'} class="draw" />
							</svg>
						</span>
					</div>
				{/if}
			</div>
			<div class="mx-[-6%] h-1.5 rounded-b-md bg-zinc-300 dark:bg-zinc-700"></div>
		</div>

		<!-- The cable: drawn as it is plugged in, then requests out and signed answers back. -->
		<div class="relative mb-[9%] flex-1">
			<svg viewBox="0 0 100 30" preserveAspectRatio="none" class="h-8 w-full overflow-visible">
				<path
					d="M0 14 Q50 32 100 14"
					pathLength="100"
					fill="none"
					stroke-width="2"
					vector-effect="non-scaling-stroke"
					class="cable stroke-zinc-400 dark:stroke-zinc-500"
					class:plugging={scene === 0}
				/>
				{#if scene === 2 || scene === 3}
					<path
						d="M0 14 Q50 32 100 14"
						pathLength="100"
						vector-effect="non-scaling-stroke"
						class="packet out stroke-zinc-500 dark:stroke-zinc-300"
						class:fast={scene === 3}
					/>
					{#if scene === 2}
						<path
							d="M0 14 Q50 32 100 14"
							pathLength="100"
							vector-effect="non-scaling-stroke"
							class="packet back stroke-yellow-400"
						/>
					{/if}
				{/if}
			</svg>
			<!-- The lock, open once the board has proved itself. -->
			<!-- overflow-visible: the open shackle rises past the viewBox top. -->
			<svg
				viewBox="-12 -14 24 32"
				class="absolute -top-9 left-1/2 w-6 -translate-x-1/2 overflow-visible"
			>
				<path
					d="M-7 0v-6a7 7 0 0 1 14 0v6"
					fill="none"
					stroke-width="2.5"
					stroke-linecap="round"
					class="shackle stroke-zinc-400 transition-transform duration-500 dark:stroke-zinc-500"
					class:open={scene >= 3 && !failed}
					class:unlocking={scene === 2}
				/>
				<rect
					x="-11"
					width="22"
					height="17"
					rx="4"
					class="transition-colors duration-500 {failed
						? 'fill-red-500'
						: scene >= 3
							? 'fill-yellow-400'
							: 'fill-zinc-300 dark:fill-zinc-700'}"
				/>
				<circle cy="8" r="2.5" class="fill-white dark:fill-zinc-900" />
			</svg>
		</div>

		<!-- The device: a small body with an inset screen, a button and its status LED.
			mb-[6%] keeps the USB port level with the cable end. -->
		<div
			class="relative mb-[6%] flex aspect-[4/3] w-[20%] shrink-0 flex-col gap-[8%] rounded-md border-2 border-zinc-300 bg-zinc-100 p-[8%] dark:border-zinc-700 dark:bg-zinc-800"
		>
			<span
				class="absolute top-1/2 -left-[11%] h-[24%] w-[9%] -translate-y-1/2 rounded-l-sm bg-zinc-400 dark:bg-zinc-500"
			></span>
			<span
				class="flex flex-1 flex-col justify-center gap-[12%] rounded-sm bg-zinc-900 px-[10%] ring-1 ring-zinc-300 dark:bg-zinc-950 dark:ring-zinc-700"
			>
				<span class="block h-[10%] w-1/2 rounded-full bg-zinc-600"></span>
				<span class="block h-[10%] w-1/3 rounded-full bg-zinc-700"></span>
			</span>
			<span class="flex items-center justify-between">
				<span class="h-1 w-1/4 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
				<span
					class="led size-1.5 rounded-full transition-colors duration-300 {failed
						? 'bg-red-500'
						: scene === 4
							? 'bg-emerald-400'
							: 'bg-yellow-400'}"
					class:off={scene === 0}
					class:blink={scene === 2}
					class:busy={scene === 3}
				></span>
			</span>
		</div>
	</div>

	<div class="relative mt-5 min-h-12 text-center" aria-live="polite">
		{#key caption[0]}
			<div in:fly={{ y: 6, duration: 250 }}>
				<h2 class="font-semibold">{caption[0]}</h2>
				<p class="mx-auto max-w-md text-sm text-zinc-500">{caption[1]}</p>
			</div>
		{/key}
	</div>

	<ol class="relative mt-5 grid gap-2 sm:grid-cols-4">
		{#each STEPS as [title, text], i (title)}
			{@const state =
				failed && stepOf(scene) === i
					? 'failed'
					: stepOf(scene) > i
						? 'done'
						: stepOf(scene) === i
							? 'now'
							: 'next'}
			<li>
				<button
					type="button"
					onclick={() => pick(i)}
					disabled={!!live}
					aria-current={state === 'now' ? 'step' : undefined}
					class="flex h-full w-full gap-3 rounded-lg p-3 text-left transition duration-300 enabled:cursor-pointer sm:flex-col sm:gap-2 {state ===
					'now'
						? 'bg-yellow-400/10 ring-2 ring-yellow-400'
						: state === 'failed'
							? 'bg-red-500/10 ring-2 ring-red-500'
							: 'bg-zinc-50 enabled:hover:bg-zinc-100 dark:bg-zinc-950/60 dark:enabled:hover:bg-zinc-950'}"
				>
					<span
						class="grid size-6 shrink-0 place-items-center rounded-full text-xs font-bold transition-colors duration-300 {state ===
						'done'
							? 'bg-emerald-500 text-white'
							: state === 'failed'
								? 'bg-red-500 text-white'
								: state === 'now'
									? 'bg-yellow-400 text-zinc-950'
									: 'bg-zinc-200 text-zinc-500 dark:bg-zinc-800'}"
					>
						{#if state === 'done'}
							<svg
								viewBox="0 0 16 16"
								class="size-3.5"
								fill="none"
								stroke="currentColor"
								stroke-width="2.5"
								stroke-linecap="round"
								stroke-linejoin="round"><path d="M3.5 8.5l3 3 6-7" /></svg
							>
						{:else}{i + 1}{/if}
					</span>
					<span class="space-y-0.5">
						<span class="block text-sm font-semibold">{title}</span>
						<span class="block text-xs text-zinc-500">{text}</span>
					</span>
				</button>
			</li>
		{/each}
	</ol>
</div>

<style>
	/* Everything that loops or plays is under .motion, which reduced motion leaves off. */
	.cable {
		stroke-dasharray: 100;
		stroke-dashoffset: 0;
	}
	.motion .cable.plugging {
		animation: plug 1.4s ease-in-out both;
	}
	@keyframes plug {
		from {
			stroke-dashoffset: 100;
		}
	}
	.motion .led.off {
		animation: power 3s steps(1) both;
	}
	@keyframes power {
		0% {
			opacity: 0.2;
		}
		50% {
			opacity: 1;
		}
	}
	.motion .led.blink {
		animation: blink 1.6s ease-in-out infinite;
	}
	.motion .led.busy {
		animation: blink 0.35s steps(2) infinite;
	}
	@keyframes blink {
		50% {
			opacity: 0.25;
		}
	}

	.packet {
		fill: none;
		stroke-width: 4;
		stroke-linecap: round;
		stroke-dasharray: 12 88;
		stroke-dashoffset: 12;
		opacity: 0;
	}
	.motion .packet.out {
		animation: out 1.6s ease-in-out infinite;
	}
	.motion .packet.out.fast {
		animation-duration: 0.7s;
		animation-timing-function: linear;
	}
	.motion .packet.back {
		animation: back 1.6s ease-in-out infinite;
	}
	@keyframes out {
		0% {
			stroke-dashoffset: 12;
			opacity: 1;
		}
		45% {
			stroke-dashoffset: -88;
			opacity: 1;
		}
		46%,
		100% {
			opacity: 0;
		}
	}
	@keyframes back {
		0%,
		50% {
			stroke-dashoffset: -88;
			opacity: 0;
		}
		51% {
			opacity: 1;
		}
		95% {
			stroke-dashoffset: 12;
			opacity: 1;
		}
		96%,
		100% {
			opacity: 0;
		}
	}

	.shackle.open {
		transform: translateY(-4px);
	}
	.motion .shackle.unlocking {
		animation: jiggle 1.6s ease-in-out infinite;
	}
	@keyframes jiggle {
		85%,
		95% {
			transform: translateY(-2px);
		}
	}

	/* The popup: the pointer comes in, picks the device row, then clicks Connect. */
	.row {
		color: var(--color-zinc-500);
	}
	.motion .row {
		animation: row 3s ease both;
	}
	@keyframes row {
		0%,
		30% {
			background: transparent;
		}
		38%,
		100% {
			background: color-mix(in oklab, var(--color-yellow-400) 30%, transparent);
			color: inherit;
		}
	}
	.motion .connect {
		animation: connect 3s ease both;
	}
	@keyframes connect {
		0%,
		38% {
			background: var(--color-zinc-300);
		}
		45%,
		100% {
			background: var(--color-yellow-400);
			color: var(--color-zinc-950);
		}
		78% {
			transform: none;
		}
		82% {
			transform: scale(0.9);
		}
		88% {
			transform: none;
		}
	}
	.cursor {
		right: 6%;
		bottom: -40%;
		opacity: 0;
	}
	.motion .cursor {
		animation: cursor 3s ease-in-out both;
	}
	@keyframes cursor {
		0% {
			right: 4%;
			bottom: -40%;
			opacity: 0;
		}
		12% {
			opacity: 1;
		}
		32%,
		45% {
			right: 70%;
			bottom: 34%;
		}
		75%,
		88% {
			right: 4%;
			bottom: 4%;
			opacity: 1;
		}
		82% {
			transform: scale(0.85);
		}
		100% {
			right: 4%;
			bottom: 4%;
			opacity: 1;
		}
	}

	.motion .spin {
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	.draw {
		stroke-dasharray: 20;
	}
	.motion .draw {
		animation: draw 0.5s 0.15s ease-out both;
	}
	@keyframes draw {
		from {
			stroke-dashoffset: 20;
		}
	}
	.motion .glow {
		animation: glow 3s ease-in-out infinite;
	}
	@keyframes glow {
		50% {
			opacity: 0.6;
		}
	}
</style>
