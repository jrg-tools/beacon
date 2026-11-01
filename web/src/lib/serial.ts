// Talks to a board over USB with Web Serial (Chrome and Edge on desktop), one command per line.
// The commands are the firmware's BeaconSerial.h; replies start with "beacon:" amid the board's log.

// Web Serial is not in TypeScript's DOM types; just what this file uses.
export interface SerialPort {
	readonly readable: ReadableStream<Uint8Array>;
	readonly writable: WritableStream<Uint8Array>;
	open(options: { baudRate: number }): Promise<void>;
	setSignals(signals: { dataTerminalReady?: boolean; requestToSend?: boolean }): Promise<void>;
	close(): Promise<void>;
}

export const hasSerial = () => typeof navigator !== 'undefined' && 'serial' in navigator;

/** Asks the browser for a port; rejects with a `NotFoundError` DOMException if none is picked. */
export async function connect() {
	const serial = (
		navigator as Navigator & {
			serial: { requestPort(options: { filters: { usbVendorId: number }[] }): Promise<SerialPort> };
		}
	).serial;
	// Only ESP32 boards: Espressif's native USB, plus the USB-UART chips dev boards use (CP210x, CH34x, FTDI).
	const port = await serial.requestPort({
		filters: [0x303a, 0x10c4, 0x1a86, 0x0403].map((usbVendorId) => ({ usbVendorId }))
	});
	await port.open({ baudRate: 115200 });
	// The ESP32-S3's USB port resets the chip when RTS is on and DTR off, so RTS goes first.
	await port.setSignals({ requestToSend: false });
	await port.setSignals({ dataTerminalReady: false });
	const reader = port.readable.getReader();
	const writer = port.writable.getWriter();
	const decoder = new TextDecoder();
	let buffer = '';
	let pending: Promise<ReadableStreamReadResult<Uint8Array>> | null = null;

	async function readLine(until: number): Promise<string | null> {
		for (;;) {
			const end = buffer.indexOf('\n');
			if (end >= 0) {
				const line = buffer.slice(0, end).trim();
				buffer = buffer.slice(end + 1);
				return line;
			}
			const left = until - Date.now();
			if (left <= 0) return null;
			// A read can't be cancelled without closing the stream, so an unfinished one carries over.
			pending ??= reader.read();
			const result = await Promise.race([
				pending,
				new Promise<null>((resolve) => setTimeout(() => resolve(null), left))
			]);
			if (!result) return null;
			pending = null;
			if (result.done) throw new Error('The device disconnected.');
			buffer += decoder.decode(result.value, { stream: true });
		}
	}

	return {
		/** For esptool once this session is closed: it reopens the port itself. */
		port,
		/**
		 * Sends `command` every 2 s (a board that was just reset misses it) until a line holding
		 * `reply` arrives, and returns what follows `reply`. Commands must be safe to repeat.
		 */
		async ask(command: string, reply: string, timeoutMs = 15000): Promise<string> {
			const deadline = Date.now() + timeoutMs;
			while (Date.now() < deadline) {
				await writer.write(new TextEncoder().encode(`${command}\n`));
				const resendAt = Math.min(Date.now() + 2000, deadline);
				for (let line; (line = await readLine(resendAt)) !== null;) {
					const at = line.indexOf(reply);
					if (at >= 0) return line.slice(at + reply.length).trim();
				}
			}
			throw new Error('The device did not answer: is it running firmware with Beacon sync?');
		},
		async close() {
			await reader.cancel().catch(() => {});
			reader.releaseLock();
			writer.releaseLock();
			await port.close().catch(() => {});
		}
	};
}
