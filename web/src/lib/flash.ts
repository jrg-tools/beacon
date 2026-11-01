// Writes a release onto a board over USB with esptool-js, keeping its NVS (WiFi, Beacon key and id).
// Where each image goes comes from the board's own partition table, which the running app reports
// (esptool-js's readFlash stalls on the ESP32-S3's USB port), so one page fits every project.

import { ESPLoader, Transport } from 'esptool-js';
import type { SerialPort } from './serial.ts';

export interface Partition {
	type: number;
	subtype: number;
	offset: number;
	size: number;
	label: string;
}

/** The firmware's `beacon partitions` reply: "type,subtype,offset,size,label;…", numbers in hex but the first two. */
export function partitions(reply: string): Partition[] {
	return reply
		.split(';')
		.filter(Boolean)
		.map((entry) => {
			const [type, subtype, offset, size, label = ''] = entry.split(',');
			return {
				type: Number(type),
				subtype: Number(subtype),
				offset: parseInt(offset, 16),
				size: parseInt(size, 16),
				label
			};
		})
		.filter((p) => [p.type, p.subtype, p.offset, p.size].every(Number.isFinite));
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
	table: Partition[],
	assets: { name: string; data: Uint8Array }[],
	log: (line: string) => void,
	progress: (fraction: number) => void
) {
	// Placed before the bootloader is entered, so a release that doesn't fit never touches the board.
	const { files, skipped } = place(table, assets);
	for (const name of skipped) log(`Skipping ${name}: not an app or filesystem image.`);
	const transport = new Transport(port as never);
	const loader = new ESPLoader({
		transport,
		baudrate: 921600,
		terminal: { clean() {}, writeLine: log, write: log }
	});
	try {
		log(`Chip: ${await loader.main()}`);
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
	} finally {
		// Even after a failure: left in the bootloader, the board looks dead until it is replugged.
		log('Restarting the board…');
		await loader.after('hard_reset').catch(() => {});
		await transport.disconnect().catch(() => {});
	}
}
