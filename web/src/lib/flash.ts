// Talks to a board's ROM bootloader over USB with esptool-js, whatever (if anything) the board runs:
// which chip it is, its partition table and otadata, which app sits in each slot (`inspect`), and
// installs, full backups, restores and switching back to the previous app.
// Where each image goes comes from the board's own table, so one page fits every project; a first
// flash takes it from the release's partitions.bin instead. Every image is checked the way the
// bootloader will before the board is touched, nothing is written to a board of another chip, and
// every write is read back (MD5).

import { ESPLoader, Transport } from 'esptool-js';
import type { SerialPort } from './serial.ts';

export interface Partition {
	type: number;
	subtype: number;
	offset: number;
	size: number;
	label: string;
}

type Asset = { name: string; data: Uint8Array };
type File = Asset & { address: number };
type Log = (line: string) => void;

const SECTOR = 0x1000;

// esptool's IMAGE_CHIP_ID of each chip, for naming what an image is built for.
const CHIPS: Record<number, string> = {
	0: 'ESP32',
	2: 'ESP32-S2',
	5: 'ESP32-C3',
	9: 'ESP32-S3',
	12: 'ESP32-C2',
	13: 'ESP32-C6',
	16: 'ESP32-H2',
	18: 'ESP32-P4',
	23: 'ESP32-C5'
};
export const chipName = (id: number) => CHIPS[id] ?? `chip ${id}`;

// Where the bootloader sits; 0x0 on every other chip.
const BOOTLOADER: Record<number, number> = { 0: 0x1000, 2: 0x1000, 18: 0x2000 };

const FS = /filesystem|littlefs|spiffs/i;
const APP = /firmware.*\.bin$/i;

export const hex = (bytes: Uint8Array) =>
	Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');

const MD5_K = Array.from({ length: 64 }, (_, i) => Math.floor(Math.abs(Math.sin(i + 1)) * 2 ** 32));
const MD5_S = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];

/** MD5 (RFC 1321), which WebCrypto lacks: the stub's and the partition table's checksum. */
export function md5(data: Uint8Array) {
	const padded = new Uint8Array((Math.floor((data.length + 8) / 64) + 1) * 64);
	padded.set(data);
	padded[data.length] = 0x80;
	const view = new DataView(padded.buffer);
	view.setUint32(padded.length - 8, (data.length * 8) >>> 0, true);
	view.setUint32(padded.length - 4, Math.floor(data.length / 2 ** 29), true);
	const h = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
	for (let at = 0; at < padded.length; at += 64) {
		let [a, b, c, d] = h;
		for (let i = 0; i < 64; i++) {
			const round = i >> 4;
			const f =
				round === 0
					? (b & c) | (~b & d)
					: round === 1
						? (d & b) | (~d & c)
						: round === 2
							? b ^ c ^ d
							: c ^ (b | ~d);
			const g = [i, 5 * i + 1, 3 * i + 5, 7 * i][round] % 16;
			const x = (a + f + MD5_K[i] + view.getUint32(at + g * 4, true)) | 0;
			const s = MD5_S[(i >> 4) * 4 + (i & 3)];
			a = d;
			d = c;
			c = b;
			b = (b + ((x << s) | (x >>> (32 - s)))) | 0;
		}
		h[0] = (h[0] + a) | 0;
		h[1] = (h[1] + b) | 0;
		h[2] = (h[2] + c) | 0;
		h[3] = (h[3] + d) | 0;
	}
	const out = new Uint8Array(16);
	h.forEach((v, i) => new DataView(out.buffer).setUint32(i * 4, v, true));
	return out;
}

/** The chip an app or bootloader image is built for (bytes 12-13 of its header); undefined if not one. */
export function imageChip({ name, data }: Asset) {
	if (FS.test(name) || data.length < 24 || data[0] !== 0xe9) return;
	return data[12] | (data[13] << 8);
}

/**
 * Checks an app or bootloader image as the bootloader will: header, segments, checksum and the
 * appended SHA-256. Throws naming what is wrong. `exact`: nothing may follow the image (false for
 * one inside a backup). Returns its length.
 */
export async function checkImage(name: string, data: Uint8Array, exact = true) {
	const fail = (why: string): never => {
		throw new Error(`${name} is not a valid firmware image: ${why}.`);
	};
	if (data.length < 24 || data[0] !== 0xe9) fail('no ESP image header (0xE9)');
	const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
	let at = 24;
	let sum = 0xef;
	for (let i = 0; i < data[1]; i++) {
		if (at + 8 > data.length) fail('a segment header runs past its end');
		const length = view.getUint32(at + 4, true);
		at += 8;
		if (length > data.length - at) fail('a segment runs past its end');
		for (let k = at; k < at + length; k++) sum ^= data[k];
		at += length;
	}
	// Padded so the checksum byte ends a 16-byte block, then the SHA-256 if the header says so.
	const padded = (at + 16) & ~15;
	const end = padded + (data[23] === 1 ? 32 : 0);
	if (end > data.length || (exact && end !== data.length))
		fail(`its segments make ${end} bytes but the file has ${data.length}`);
	if (data[padded - 1] !== sum) fail('checksum mismatch');
	if (data[23] === 1) {
		const sha = await crypto.subtle.digest('SHA-256', data.slice(0, padded));
		if (hex(new Uint8Array(sha)) !== hex(data.subarray(padded, end))) fail('SHA-256 mismatch');
	}
	return end;
}

/**
 * A binary partition table (partitions.bin), read the way the bootloader does: 32-byte entries
 * starting 0xAA 0x50, ended by the MD5 entry or blank flash. Empty if `data` is not one.
 */
export function tableFromImage(data: Uint8Array): Partition[] {
	const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
	const table: Partition[] = [];
	for (let at = 0; at + 32 <= data.length && view.getUint16(at) === 0xaa50; at += 32)
		table.push({
			type: data[at + 2],
			subtype: data[at + 3],
			offset: view.getUint32(at + 4, true),
			size: view.getUint32(at + 8, true),
			label: new TextDecoder().decode(data.subarray(at + 12, at + 28)).replace(/\0.*$/s, '')
		});
	return table;
}

/** True unless the table's MD5 entry (if it has one: the default build) disagrees with its entries. */
export function tableIntact(data: Uint8Array) {
	const at = tableFromImage(data).length * 32;
	if (at + 32 > data.length || data[at] !== 0xeb || data[at + 1] !== 0xeb) return true;
	return hex(md5(data.subarray(0, at))) === hex(data.subarray(at + 16, at + 32));
}

/**
 * What a release's files must pass before anything connects: app and bootloader images
 * (`checkImage`), partition tables (`tableIntact`). Other files (filesystem, boot_app0) pass as they are.
 */
export async function checkFiles(files: Asset[]) {
	for (const { name, data } of files) {
		if (FS.test(name)) continue;
		if (tableFromImage(data).length) {
			if (!tableIntact(data))
				throw new Error(`${name} is a partition table that fails its MD5 check.`);
		} else if (data[0] === 0xe9 || APP.test(name)) await checkImage(name, data);
	}
}

/** The app description ESP-IDF puts after an app's header (esp_app_desc_t), from its first 256 bytes. */
export function appDesc(head: Uint8Array) {
	if (head.length < 0xd0 || head[0] !== 0xe9) return;
	const view = new DataView(head.buffer, head.byteOffset, head.byteLength);
	if (view.getUint32(32, true) !== 0xabcd5432) return;
	const text = (at: number, length: number) =>
		new TextDecoder().decode(head.subarray(at, at + length)).replace(/\0.*$/s, '');
	return {
		version: text(48, 32),
		project: text(80, 32),
		built: `${text(128, 16)} ${text(112, 16)}`,
		/** The ELF's SHA-256, which the server reads from each release's app to name it (`app_sha`). */
		sha: hex(head.subarray(0xb0, 0xd0))
	};
}

const otaApps = (table: Partition[]) =>
	table
		.filter((p) => p.type === 0 && p.subtype >= 0x10 && p.subtype < 0x20)
		.sort((a, b) => a.subtype - b.subtype);
const factoryApp = (table: Partition[]) => table.find((p) => p.type === 0 && p.subtype === 0x00);

// otadata's CRC of its sequence number: esp_rom_crc32_le(UINT32_MAX, …), which inverts its seed.
export function otaCrc(seq: number) {
	let crc = 0;
	for (let i = 0; i < 4; i++) {
		crc ^= (seq >>> (8 * i)) & 0xff;
		for (let k = 0; k < 8; k++) crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
	}
	return ~crc >>> 0;
}

// otadata's live entry, as the bootloader picks it: the valid one (one per sector) with the highest
// sequence number. Undefined when blank, so the bootloader starts factory, else ota_0.
function activeEntry(otadata: Uint8Array) {
	const entries = [0, 1].flatMap((sector) => {
		const view = new DataView(otadata.buffer, otadata.byteOffset + sector * SECTOR, 32);
		const seq = view.getUint32(0, true);
		const state = view.getUint32(24, true); // 3 invalid, 4 aborted
		const valid = seq !== 0xffffffff && view.getUint32(28, true) === otaCrc(seq);
		return valid && state !== 3 && state !== 4 ? [{ seq, sector }] : [];
	});
	return entries.sort((a, b) => b.seq - a.seq)[0];
}

/** The app the bootloader starts given `otadata` (both of its sectors, as read from the board). */
export function bootApp(table: Partition[], otadata: Uint8Array) {
	const ota = otaApps(table);
	const active = activeEntry(otadata);
	return active && ota.length ? ota[(active.seq - 1) % ota.length] : (factoryApp(table) ?? ota[0]);
}

/** otadata's sector (as a file to write) whose new entry makes the bootloader start `app`; the live entry stays. */
function select(table: Partition[], current: Uint8Array, app: Partition): File {
	const ota = otaApps(table);
	const otadata = table.find((p) => p.type === 1 && p.subtype === 0x00)!;
	const active = activeEntry(current);
	let seq = (active?.seq ?? 0) + 1;
	while ((seq - 1) % ota.length !== ota.indexOf(app)) seq++;
	const data = new Uint8Array(SECTOR).fill(0xff);
	const view = new DataView(data.buffer);
	view.setUint32(0, seq, true);
	view.setUint32(28, otaCrc(seq), true);
	const sector = active ? 1 - active.sector : 0;
	return { name: 'otadata', data, address: otadata.offset + sector * SECTOR };
}

/**
 * Where each asset goes: the filesystem image on the SPIFFS/LittleFS partition, the app on the OTA
 * slot after the running one when the board's `otadata` is given (as its own WiFi updates do, so
 * the running app stays intact), else on the one the bootloader picks once otadata is blank
 * (factory, else ota_0). Other assets are skipped: rewriting the bootloader or the table could
 * brick the board.
 */
export function place(table: Partition[], assets: Asset[], current?: Uint8Array) {
	const ota = otaApps(table);
	const otadata = table.find((p) => p.type === 1 && p.subtype === 0x00);
	const next =
		current &&
		current.length >= 2 * SECTOR &&
		otadata &&
		otadata.size >= 2 * SECTOR &&
		ota.length > 1
			? ota[(ota.indexOf(bootApp(table, current)) + 1) % ota.length]
			: undefined;
	const app = next ?? factoryApp(table) ?? ota[0];
	const fs = table.find((p) => p.type === 1 && (p.subtype === 0x82 || p.subtype === 0x83));
	const files: File[] = [];
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
		// Below 0x9000 are the bootloader and the table: a table that points there is corrupt.
		if (target.offset < 0x9000)
			throw new Error(`The "${target.label}" partition overlaps the bootloader: corrupt table?`);
		files.push({ name, data, address: target.offset });
	}
	if (!files.some((f) => f.address === app?.offset))
		throw new Error('This release has no app image (an asset named like firmware.bin).');
	if (next) files.push(select(table, current!, next));
	else if (otadata)
		// Blank otadata makes the bootloader start the partition just written, not the last OTA one.
		files.push({
			name: 'otadata',
			data: new Uint8Array(otadata.size).fill(0xff),
			address: otadata.offset
		});
	return { files, skipped, app: app! };
}

/** A project's `first_flash` ("bootloader.bin@0x0,partitions.bin@0x8000", as the server normalised it). */
export function extras(firstFlash: string) {
	return firstFlash
		.split(',')
		.filter(Boolean)
		.map((entry) => {
			const [name, offset] = entry.split('@');
			return { name, address: parseInt(offset, 16) };
		});
}

/**
 * A first flash's files: the extras at their offsets, then the assets placed with the partition
 * table among the extras. Throws if an extra is missing or none is a partition table.
 */
export function firstFlashFiles(
	firstFlash: { name: string; address: number }[],
	downloaded: Asset[],
	tag: string
) {
	const files: File[] = [];
	const assets: Asset[] = [];
	for (const asset of downloaded) {
		const extra = firstFlash.find((e) => e.name === asset.name);
		if (extra) files.push({ ...asset, address: extra.address });
		else assets.push(asset);
	}
	const missing = firstFlash.filter((e) => !files.some((f) => f.name === e.name));
	if (missing.length) throw new Error(`${tag} has no ${missing.map((e) => e.name).join(', ')}.`);
	const table = files.map((f) => tableFromImage(f.data)).find((t) => t.length) ?? [];
	if (!table.length)
		throw new Error(
			'No first-flash file is a partition table: add partitions.bin@0x8000 on the project.'
		);
	const placed = place(table, assets);
	files.push(...placed.files.filter((p) => !files.some((f) => f.address === p.address)));
	return { files, table, skipped: placed.skipped };
}

// esptool-js's readFlash asks for 1024 packets in flight and waits 100 s for each, which stalled on
// the ESP32-S3's USB port: this asks for small ones, acks each, fails instead of hanging, and checks
// the stub's MD5 of what it read.
async function read(loader: ESPLoader, addr: number, size: number) {
	const args = new Uint8Array(16);
	const view = new DataView(args.buffer);
	[addr, size, 0x100, 1].forEach((n, i) => view.setUint32(i * 4, n, true));
	await loader.checkCommand('read flash', loader.ESP_READ_FLASH, args);
	const data = new Uint8Array(size);
	for (let got = 0; got < size;) {
		const packet = await loader.transport.read(3000);
		data.set(packet.subarray(0, size - got), got);
		got += packet.length;
		await loader.transport.write(loader._intToByteArray(got));
	}
	const digest = await loader.transport.read(3000);
	if (hex(digest.subarray(0, 16)) !== hex(md5(data)))
		throw new Error(`Reading flash at 0x${addr.toString(16)} failed its MD5 check.`);
	return data;
}

/** What `inspect` finds on a board: everything an install needs, read from flash. */
export type Board = Awaited<ReturnType<typeof inspect>>;

// The table at 0x8000 (empty on a blank board), otadata, the app the bootloader starts and the
// header of every app slot: chip and esp_app_desc_t, so the page names what is installed.
async function inspect(loader: ESPLoader) {
	const raw = await read(loader, 0x8000, SECTOR);
	const table = tableFromImage(raw);
	if (!tableIntact(raw))
		throw new Error(
			'The partition table on the board fails its MD5 check: it needs a first flash.'
		);
	const part = table.find((p) => p.type === 1 && p.subtype === 0x00);
	const otadata =
		part && part.size >= 2 * SECTOR ? await read(loader, part.offset, 2 * SECTOR) : undefined;
	const running = otadata ? bootApp(table, otadata) : (factoryApp(table) ?? otaApps(table)[0]);
	const apps = [];
	for (const partition of table.filter((p) => p.type === 0)) {
		const head = await read(loader, partition.offset, 0x100);
		apps.push({
			partition,
			chip: imageChip({ name: partition.label, data: head }),
			desc: appDesc(head)
		});
	}
	return {
		chip: loader.chip.CHIP_NAME,
		chipId: loader.chip.IMAGE_CHIP_ID,
		table,
		otadata,
		running,
		apps
	};
}

// Runs `work` with the board in its ROM bootloader (the flasher stub loaded), then restarts it
// into its app, even after a failure.
async function session<T>(port: SerialPort, log: Log, work: (loader: ESPLoader) => Promise<T>) {
	const transport = new Transport(port as never);
	const loader = new ESPLoader({
		transport,
		baudrate: 921600,
		terminal: { clean() {}, writeLine: log, write: log }
	});
	try {
		log(`Chip: ${await loader.main()}`);
		return await work(loader);
	} finally {
		// Left in the bootloader, the board looks dead until it is replugged.
		log('Restarting the board…');
		// esptool-js's 'hard_reset' only releases RTS without pulling it first, so the chip never
		// resets and stays in the flasher (a board with a battery then looks dead). So: IO0 high
		// (DTR off) for a normal boot, then pulse EN low (RTS on) and release it.
		await loader.after('custom_reset', undefined, 'D0|R1|W100|R0').catch(() => {});
		await transport.disconnect().catch(() => {});
	}
}

function expectChip(loader: ESPLoader, files: Asset[]) {
	const { CHIP_NAME, IMAGE_CHIP_ID } = loader.chip;
	for (const f of files) {
		const id = imageChip(f);
		if (id !== undefined && IMAGE_CHIP_ID !== undefined && id !== IMAGE_CHIP_ID)
			throw new Error(
				`Connected device is an ${CHIP_NAME}, but ${f.name} is built for the ${chipName(id)}. Wrong device? Flashing was aborted before anything was written.`
			);
	}
}

// Writes `files` and checks each against the chip's MD5 of what landed in flash.
async function write(
	loader: ESPLoader,
	files: File[],
	progress: (f: number) => void,
	eraseAll = false
) {
	const total = files.reduce((sum, f) => sum + f.data.length, 0);
	const before = files.map((_, i) => files.slice(0, i).reduce((sum, f) => sum + f.data.length, 0));
	await loader.writeFlash({
		fileArray: files,
		flashMode: 'keep',
		flashFreq: 'keep',
		flashSize: 'keep',
		eraseAll,
		compress: true,
		calculateMD5Hash: (image) => hex(md5(image)),
		reportProgress: (i, written, size) =>
			progress((before[i] + (written / size) * files[i].data.length) / total)
	});
}

async function flashSize(loader: ESPLoader) {
	const size = await loader.detectFlashSize();
	if (!size) throw new Error("Could not read the size of the board's flash.");
	return loader.flashSizeBytes(size as Parameters<ESPLoader['flashSizeBytes']>[0]);
}

/** The board as its bootloader sees it (`inspect`), touching nothing. */
export function probe(port: SerialPort, log: Log) {
	return session(port, log, inspect);
}

/**
 * Writes `plan`: fixed files (a first flash's), or assets placed with the board's own table and
 * otadata, on the app slot it is not running. Every file is checked first (`checkFiles`), before
 * the board is touched; nothing is written to a board of another chip or whose assets don't fit.
 * `eraseAll` wipes NVS too: first flash only.
 */
export async function flash(
	port: SerialPort,
	plan: { files: File[] } | { assets: Asset[] },
	log: Log,
	progress: (fraction: number) => void,
	eraseAll = false
) {
	await checkFiles('files' in plan ? plan.files : plan.assets);
	await session(port, log, async (loader) => {
		expectChip(loader, 'files' in plan ? plan.files : plan.assets);
		if ('files' in plan) {
			for (const f of plan.files) log(`Writing ${f.name} at 0x${f.address.toString(16)}…`);
			return write(loader, plan.files, progress, eraseAll);
		}
		const board = await inspect(loader);
		if (!board.table.length)
			throw new Error(
				'The board has no partition table: it needs a first flash. Nothing was written.'
			);
		const { files, skipped, app } = place(board.table, plan.assets, board.otadata);
		for (const name of skipped) log(`Skipping ${name}: not an app or filesystem image.`);
		log(`The app goes on ${app.label}, which the board will boot.`);
		for (const f of files) log(`Writing ${f.name} at 0x${f.address.toString(16)}…`);
		await write(loader, files, progress, eraseAll);
		if (board.otadata) {
			const part = board.table.find((p) => p.type === 1 && p.subtype === 0x00)!;
			const booted = bootApp(board.table, await read(loader, part.offset, 2 * SECTOR));
			if (booted?.offset !== app.offset)
				throw new Error(
					`otadata did not verify: the board would boot ${booted?.label}, not ${app.label}.`
				);
		}
	});
}

/** The whole flash, read back in 64 KB chunks (each MD5-checked, retried up to 3 times). */
export function backup(port: SerialPort, log: Log, progress: (fraction: number) => void) {
	return session(port, log, async (loader) => {
		const size = await flashSize(loader);
		log(`Reading ${size / 0x100000} MB of flash…`);
		const image = new Uint8Array(size);
		const CHUNK = 0x10000;
		for (let at = 0; at < size; at += CHUNK) {
			for (let attempt = 1; ; attempt++) {
				try {
					image.set(await read(loader, at, Math.min(CHUNK, size - at)), at);
					break;
				} catch (err) {
					if (attempt === 3) throw err;
					log(`Retrying at 0x${at.toString(16)}: ${(err as Error).message}`);
					await loader.transport.drainInput();
				}
			}
			progress(Math.min(at + CHUNK, size) / size);
		}
		return { image, chip: loader.chip.CHIP_NAME };
	});
}

/**
 * Writes a `backup` back whole. It must be this board's size, start with a valid bootloader for its
 * chip and hold an intact partition table at 0x8000; checked before anything is written.
 */
export function restore(
	port: SerialPort,
	image: Uint8Array,
	log: Log,
	progress: (fraction: number) => void
) {
	return session(port, log, async (loader) => {
		const size = await flashSize(loader);
		if (image.length !== size)
			throw new Error(
				`The backup is ${image.length} bytes but this board's flash is ${size}: not its backup.`
			);
		const at = BOOTLOADER[loader.chip.IMAGE_CHIP_ID ?? -1] ?? 0;
		const bootloader = { name: "The backup's bootloader", data: image.subarray(at) };
		await checkImage(bootloader.name, bootloader.data, false);
		expectChip(loader, [bootloader]);
		const table = image.subarray(0x8000, 0x8000 + SECTOR);
		if (!tableFromImage(table).length || !tableIntact(table))
			throw new Error('The backup has no intact partition table at 0x8000. Nothing was written.');
		log('Writing the backup…');
		await write(loader, [{ name: 'backup', data: image, address: 0 }], progress);
	});
}

/**
 * Makes the bootloader start the other OTA slot's app again, the one the last install replaced,
 * once its header shows an app for this chip. Returns that slot as `inspect` saw it.
 */
export function bootPrevious(port: SerialPort, log: Log) {
	return session(port, log, async (loader) => {
		const board = await inspect(loader);
		const ota = otaApps(board.table);
		const running = ota.indexOf(board.running!);
		if (ota.length < 2 || !board.otadata || running < 0)
			throw new Error('This board has no other app slot to go back to.');
		const previous = ota[(running - 1 + ota.length) % ota.length];
		const slot = board.apps.find((a) => a.partition === previous)!;
		if (slot.chip !== board.chipId)
			throw new Error(
				`${previous.label} holds no app for this ${board.chip}. Nothing was written.`
			);
		log(`Switching the boot app from ${board.running!.label} to ${previous.label}…`);
		await write(loader, [select(board.table, board.otadata, previous)], () => {});
		const part = board.table.find((p) => p.type === 1 && p.subtype === 0x00)!;
		if (
			bootApp(board.table, await read(loader, part.offset, 2 * SECTOR))?.offset !== previous.offset
		)
			throw new Error('otadata did not verify after the write.');
		return slot;
	});
}
