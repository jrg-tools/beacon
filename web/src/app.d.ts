import type { BrowserClerk } from 'svelte-clerk/client';

declare global {
	interface Window {
		/** Set by svelte-clerk's ClerkProvider once Clerk has loaded. */
		Clerk?: BrowserClerk;
	}
}

export {};
