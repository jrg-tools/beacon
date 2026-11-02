<!--
	The install page's walkthrough: plug the board in, pick it in the browser's popup, it answers, it
	updates. `signed` (a private project, or an unknown slug: the 404 is the same) is where the
	registered board is the key: it proves itself with its own key and the lock opens. Idle, it plays
	the walkthrough on its own (a step click jumps to it); during a run, `live` drives it from the
	real steps.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { fade, fly, scale } from 'svelte/transition';

	/** 0 plug in (the demo only), 1 port popup, 2 proving, 3 updating, 4 done. */
	let {
		live = null,
		signed = true
	}: {
		live?: {
			scene: number;
			progress: number;
			failed: boolean;
			tag?: string;
			/** A recovery tool's own captions for scenes 3 (busy) and 4 (done), in place of an install's. */
			captions?: [string, string][];
		} | null;
		signed?: boolean;
	} = $props();

	const STEPS = $derived([
		['Plug it in', 'Your device, over USB, on this computer.'],
		['Pick it', 'Choose it in the browser’s popup.'],
		signed
			? ['It proves itself', 'It signs with its own key, which never leaves it.']
			: ['It checks in', 'It says which device it is and what it runs.'],
		signed
			? ['It updates', 'To its newest release, and only it gets it.']
			: ['It installs', 'The release, written over USB. Blank boards work too.']
	]);
	const SCENE_MS = 3000;
	// Ends at the vertical centre (15 of 30), where the device's USB plug is.
	const CABLE = 'M0 15 Q50 32 100 15';

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

	const CAPTIONS = $derived([
		signed
			? ['This firmware is private', 'It opens with a registered device, not a password.']
			: ['Install from your browser', 'Plug your device in with a USB cable to begin.'],
		['Pick your device', 'The browser asks which USB port to use: choose your device.'],
		signed
			? ['Your device is proving itself', 'It signs the request with its own key.']
			: ['Checking your device', 'It says which device it is and which version it runs.'],
		['Installing', 'Downloading and writing the release. Keep it plugged in.'],
		['Up to date', 'Your device runs the new release, its settings intact.']
	]);
	const caption = $derived(
		failed
			? ['That didn’t work', 'See the message below, then try again.']
			: (live?.captions?.[scene - 3] ?? CAPTIONS[scene])
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
							{signed ? 'Verifying…' : 'Checking…'}
						</span>
					</div>
				{:else if scene === 3 && !failed}
					<div class="absolute inset-x-[12%] top-[52%] space-y-1" in:fade={{ duration: 250 }}>
						<span class="flex justify-between text-[8px] font-semibold text-zinc-500">
							<span
								>{live?.captions
									? live.captions[0][0]
									: live?.tag
										? `Installing ${live.tag}`
										: 'Installing'}</span
							>
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

		<!-- The cable and the device share one row, centred on the cable end and the USB port, so the
			plug always meets the cable whatever the width. -->
		<div class="mb-[12%] flex flex-1 items-center">
			<!-- The cable: drawn as it is plugged in, then requests out and signed answers back. -->
			<div class="relative flex-1">
				<svg
					viewBox="0 0 100 30"
					preserveAspectRatio="none"
					class="block h-8 w-full overflow-visible"
				>
					<path
						d={CABLE}
						pathLength="100"
						fill="none"
						stroke-width="2"
						vector-effect="non-scaling-stroke"
						class="cable stroke-zinc-400 dark:stroke-zinc-500"
						class:plugging={scene === 0}
					/>
					{#if scene === 2 || scene === 3}
						<path
							d={CABLE}
							pathLength="100"
							vector-effect="non-scaling-stroke"
							class="packet out stroke-zinc-500 dark:stroke-zinc-300"
							class:fast={scene === 3}
						/>
						{#if scene === 2}
							<path
								d={CABLE}
								pathLength="100"
								vector-effect="non-scaling-stroke"
								class="packet back stroke-yellow-400"
							/>
						{/if}
					{/if}
				</svg>
				<!-- The lock, open once the board has proved itself. -->
				<!-- overflow-visible: the open shackle rises past the viewBox top. -->
				{#if signed}<svg
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
					</svg>{/if}
			</div>

			<!-- The device: a small dev-board gadget with a landscape colour screen, two buttons on top,
				a status LED and a USB-C plug on its left edge, at the vertical centre (y 17 of 34). -->
			<!-- xMinYMid: the plug stays on the box's left edge, against the cable end. -->
			<svg
				viewBox="0 0 64 34"
				preserveAspectRatio="xMinYMid meet"
				class="block aspect-[64/34] h-auto w-[58%] shrink-0 overflow-visible"
			>
				<rect x="0" y="14" width="8" height="6" rx="1.5" class="fill-zinc-400 dark:fill-zinc-500" />
				<rect
					x="3"
					y="15.5"
					width="4"
					height="3"
					rx="0.8"
					class="fill-zinc-500 dark:fill-zinc-600"
				/>
				{#each [40, 50] as x (x)}
					<rect {x} y="0.5" width="7" height="3" rx="1" class="fill-zinc-400 dark:fill-zinc-600" />
				{/each}
				<rect
					x="6"
					y="2"
					width="57"
					height="30"
					rx="5"
					class="fill-zinc-800 stroke-zinc-900 dark:fill-zinc-950 dark:stroke-zinc-700"
					stroke-width="1"
				/>
				<!-- A sheen along the top of the case. -->
				<rect x="9" y="3.5" width="51" height="1.2" rx="0.6" class="fill-white/10" />
				<!-- The screen, its bezel, and what it shows in each scene. -->
				<rect
					x="10.5"
					y="6.5"
					width="40"
					height="21"
					rx="2"
					class="fill-zinc-950 stroke-zinc-700"
					stroke-width="0.6"
				/>
				<g class="screen" class:on={scene > 0 || failed}>
					<rect x="12" y="8" width="37" height="18" rx="1" class="fill-zinc-900" />
					{#if failed}
						<path
							d="M27 13l6 6m0-6l-6 6"
							stroke-width="1.8"
							stroke-linecap="round"
							class="stroke-red-500"
						/>
					{:else if scene === 1}
						<!-- Idle: a status bar and a couple of readings. -->
						<rect x="14" y="10" width="8" height="1.5" rx="0.75" class="fill-yellow-400" />
						<circle cx="46" cy="10.75" r="0.9" class="fill-emerald-400" />
						<rect x="14" y="15" width="20" height="3" rx="1" class="fill-zinc-600" />
						<rect x="14" y="20" width="13" height="2" rx="1" class="fill-zinc-700" />
					{:else if scene === 2 && !signed}
						<!-- Answering: an id and a version going out. -->
						<g class="sign">
							<rect x="18" y="13" width="18" height="2" rx="1" class="fill-yellow-400" />
							<rect x="18" y="18" width="11" height="2" rx="1" class="fill-zinc-600" />
						</g>
					{:else if scene === 2}
						<!-- Signing: a key. -->
						<g class="sign stroke-yellow-400" fill="none" stroke-width="1.4" stroke-linecap="round">
							<circle cx="25.5" cy="17" r="3" />
							<path d="M28.5 17h7m-2 0v2.5m-2.5-2.5v2" />
						</g>
					{:else if scene === 3}
						<rect x="16" y="16" width="29" height="2.5" rx="1.25" class="fill-zinc-700" />
						<rect
							x="16"
							y="16"
							width="29"
							height="2.5"
							rx="1.25"
							class="bar fill-yellow-400 {live ? 'duration-500' : 'duration-[2400ms]'}"
							style="transform: scaleX({fill})"
						/>
						<rect x="16" y="11" width="10" height="1.5" rx="0.75" class="fill-zinc-600" />
					{:else if scene === 4}
						<path
							d="M25 17.5l3 3 6-7"
							fill="none"
							stroke-width="1.8"
							stroke-linecap="round"
							stroke-linejoin="round"
							class="draw stroke-emerald-400"
						/>
					{/if}
				</g>
				<circle
					cx="57"
					cy="11"
					r="1.6"
					class="led transition-colors duration-300 {failed
						? 'fill-red-500'
						: scene === 4
							? 'fill-emerald-400'
							: 'fill-yellow-400'}"
					class:off={scene === 0}
					class:blink={scene === 2}
					class:busy={scene === 3}
				/>
				<!-- Speaker grille. -->
				{#each [19, 22, 25] as y (y)}
					<rect
						x="54.5"
						{y}
						width="5"
						height="1"
						rx="0.5"
						class="fill-zinc-700 dark:fill-zinc-800"
					/>
				{/each}
			</svg>
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

	.screen {
		opacity: 0.15;
		transition: opacity 0.4s;
	}
	.screen.on {
		opacity: 1;
	}
	.bar {
		transform-box: fill-box;
		transform-origin: left;
		transition-property: transform;
		transition-timing-function: ease-out;
	}
	.motion .sign {
		animation: blink 1.6s ease-in-out infinite;
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
