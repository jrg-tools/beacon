/**
 * Writes a device's credentials over USB with Web Serial (Chrome, Edge). The firmware side is in the
 * README ("Provisioning over USB"): while it listens the device prints `BEACON READY`, takes
 * `BEACON <id> <secret>` and answers `BEACON OK`.
 */
export const serialSupported = () => typeof navigator !== 'undefined' && !!navigator.serial;

export async function provision(id: string, secret: string): Promise<void> {
	const port = await navigator.serial!.requestPort();
	await port.open({ baudRate: 115200 });
	const reader = port.readable!.getReader();
	// Opening the port resets most ESP32 boards, which then listen for a few seconds after boot.
	const timer = setTimeout(() => reader.cancel(), 15000);
	try {
		await until(reader, 'BEACON READY');
		const writer = port.writable!.getWriter();
		await writer.write(new TextEncoder().encode(`BEACON ${id} ${secret}\n`));
		writer.releaseLock();
		await until(reader, 'BEACON OK');
	} finally {
		clearTimeout(timer);
		reader.releaseLock();
		await port.close();
	}
}

async function until(reader: ReadableStreamDefaultReader<Uint8Array>, text: string) {
	const decoder = new TextDecoder();
	let seen = '';
	for (;;) {
		const { value, done } = await reader.read();
		if (done) throw new Error('The device did not answer: press its reset button and try again.');
		seen += decoder.decode(value, { stream: true });
		if (seen.includes(text)) return;
	}
}
