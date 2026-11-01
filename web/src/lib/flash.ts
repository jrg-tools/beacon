// Writes a release onto a board over USB with esptool-js, keeping its NVS (WiFi, Beacon key and id).
// Where each image goes comes from the board's own partition table, so one page fits every project.

import { ESPLoader, Transport } from 'esptool-js';
import type { SerialPort } from './serial.ts';

export interface Partition {
	type: number;
	subtype: number;
	offset: number;
	size: number;
	label: string;
}

/** ESP-IDF's partition table: 32-byte entries that start with 0xAA 0x50, then an MD5 entry. */
export function partitions(table: Uint8Array): Partition[] {
	const view = new DataView(table.buffer, table.byteOffset, table.byteLength);
	const out: Partition[] = [];
	for (let at = 0; at + 32 <= table.length && view.getUint16(at, true) === 0x50aa; at += 32) {
		out.push({
			type: table[at + 2],
			subtype: table[at + 3],
			offset: view.getUint32(at + 4, true),
			size: view.getUint32(at + 8, true),
			label: new TextDecoder().decode(table.subarray(at + 12, at + 28)).replace(/\0.*$/s, '')
		});
	}
	return out;
}

/**
 * Where each asset goes: the filesystem image on the SPIFFS/LittleFS partition, the app on the
 * partition the bootloader picks once otadata is blank (factory, else ota_0). Other assets are
 * skipped: rewriting the bootloader or the table could brick the board.
 */
export function place(table: Partition[], assets: { name: string; data: Uint8Array }[]) {
	const app =
		table.find((p) => p.type === 0 && p.subtype === 0x00) ??
		table.find((p) => p.type === 0 && p.subtype === 0x10);
	const fs = table.find((p) => p.type === 1 && (p.subtype === 0x82 || p.subtype === 0x83));
	const otadata = table.find((p) => p.type === 1 && p.subtype === 0x00);
	const files: { name: string; data: Uint8Array; address: number }[] = [];
	const skipped: string[] = [];
	for (const { name, data } of assets) {
		const target = /filesystem|littlefs|spiffs/i.test(name)
			? fs
			: /firmware.*\.bin$/i.test(name)
				? app
				: undefined;
		if (!target) {
			skipped.push(name);
			continue;
		}
		if (data.length > target.size)
			throw new Error(`${name} does not fit in the "${target.label}" partition.`);
		files.push({ name, data, address: target.offset });
	}
	if (!files.some((f) => f.address === app?.offset))
		throw new Error('This release has no app image (an asset named like firmware.bin).');
	// Blank otadata makes the bootloader start the partition just written, not the last OTA one.
	if (otadata)
		files.push({
			name: 'otadata',
			data: new Uint8Array(otadata.size).fill(0xff),
			address: otadata.offset
		});
	return { files, skipped };
}

export async function flash(
	port: SerialPort,
	assets: { name: string; data: Uint8Array }[],
	log: (line: string) => void,
	progress: (fraction: number) => void
) {
	const transport = new Transport(port as never);
	const loader = new ESPLoader({
		transport,
		baudrate: 921600,
		terminal: { clean() {}, writeLine: log, write: log }
	});
	try {
		log(`Chip: ${await loader.main()}`);
		const table = partitions(await loader.readFlash(0x8000, 0xc00));
		if (!table.length) throw new Error('No partition table on the board at 0x8000.');
		for (const p of table)
			log(`Partition ${p.label} at 0x${p.offset.toString(16)} (${p.size} bytes)`);
		const { files, skipped } = place(table, assets);
		for (const name of skipped) log(`Skipping ${name}: not an app or filesystem image.`);
		const total = files.reduce((sum, f) => sum + f.data.length, 0);
		const before = files.map((_, i) =>
			files.slice(0, i).reduce((sum, f) => sum + f.data.length, 0)
		);
		for (const f of files) log(`Writing ${f.name} at 0x${f.address.toString(16)}…`);
		await loader.writeFlash({
			fileArray: files,
			flashMode: 'keep',
			flashFreq: 'keep',
			flashSize: 'keep',
			eraseAll: false,
			compress: true,
			reportProgress: (i, written, size) =>
				progress((before[i] + (written / size) * files[i].data.length) / total)
		});
		log('Restarting the board…');
		await loader.after('hard_reset');
	} finally {
		await transport.disconnect().catch(() => {});
	}
}
