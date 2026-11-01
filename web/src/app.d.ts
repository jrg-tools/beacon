import type { BrowserClerk } from 'svelte-clerk/client';

declare global {
	interface Window {
		/** Set by svelte-clerk's ClerkProvider once Clerk has loaded. */
		Clerk?: BrowserClerk;
	}

	/** The part of Web Serial that `#lib/serial.ts` uses; TypeScript's DOM types lack it. */
	interface SerialPort {
		open(options: { baudRate: number }): Promise<void>;
		close(): Promise<void>;
		readonly readable: ReadableStream<Uint8Array> | null;
		readonly writable: WritableStream<Uint8Array> | null;
	}
	interface Navigator {
		serial?: { requestPort(): Promise<SerialPort> };
	}
}

export {};
