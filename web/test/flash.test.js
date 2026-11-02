// Where flash.ts writes each file: a wrong offset bricks a board. `pnpm test` (node --test).
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import {
	appDesc,
	bootApp,
	checkFiles,
	checkImage,
	extras,
	firstFlashFiles,
	imageChip,
	md5,
	otaCrc,
	place,
	tableFromImage,
	tableIntact
} from '../src/lib/flash.ts';

// A typical 4 MB ESP32 layout: nvs, otadata, two OTA apps, SPIFFS.
const TABLE = [
	{ type: 1, subtype: 0x02, offset: 0x9000, size: 0x5000, label: 'nvs' },
	{ type: 1, subtype: 0x00, offset: 0xe000, size: 0x2000, label: 'otadata' },
	{ type: 0, subtype: 0x10, offset: 0x10000, size: 0x140000, label: 'app0' },
	{ type: 0, subtype: 0x11, offset: 0x150000, size: 0x140000, label: 'app1' },
	{ type: 1, subtype: 0x82, offset: 0x290000, size: 0x160000, label: 'spiffs' }
];

/** partitions.bin as the build writes it: 32-byte entries, then the MD5 entry, then blank flash. */
function image(table) {
	const data = new Uint8Array(0xc00).fill(0xff);
	const view = new DataView(data.buffer);
	table.forEach((p, i) => {
		const at = i * 32;
		view.setUint16(at, 0xaa50);
		data[at + 2] = p.type;
		data[at + 3] = p.subtype;
		view.setUint32(at + 4, p.offset, true);
		view.setUint32(at + 8, p.size, true);
		data.fill(0, at + 12, at + 28);
		data.set(new TextEncoder().encode(p.label), at + 12);
		view.setUint32(at + 28, 0, true);
	});
	view.setUint16(table.length * 32, 0xebeb); // the MD5 entry ends the table
	data.set(
		createHash('md5')
			.update(data.subarray(0, table.length * 32))
			.digest(),
		table.length * 32 + 16
	);
	return data;
}

const bytes = (n) => new Uint8Array(n);

test('tableFromImage reads a partitions.bin and nothing else', () => {
	assert.deepEqual(tableFromImage(image(TABLE)), TABLE);
	// A bootloader or app image is not a table.
	assert.deepEqual(tableFromImage(new Uint8Array([0xe9, 0, 0, 0]).fill(0xe9)), []);
	assert.deepEqual(tableFromImage(bytes(0)), []);
});

test('place puts the app on ota_0, the filesystem on SPIFFS and blanks otadata', () => {
	const { files, skipped } = place(TABLE, [
		{ name: 'firmware.bin', data: bytes(10) },
		{ name: 'firmware-filesystem.bin', data: bytes(20) },
		{ name: 'bootloader.bin', data: bytes(30) }
	]);
	assert.deepEqual(
		files.map((f) => [f.name, f.address]),
		[
			['firmware.bin', 0x10000],
			['firmware-filesystem.bin', 0x290000],
			['otadata', 0xe000]
		]
	);
	assert.deepEqual(files[2].data, new Uint8Array(0x2000).fill(0xff));
	// Never the bootloader or the table: that could brick the board.
	assert.deepEqual(skipped, ['bootloader.bin']);
});

test('place prefers a factory app and refuses what does not fit', () => {
	const factory = { type: 0, subtype: 0x00, offset: 0x20000, size: 0x100, label: 'factory' };
	const { files } = place([factory, ...TABLE], [{ name: 'firmware.bin', data: bytes(1) }]);
	assert.equal(files[0].address, 0x20000);
	assert.throws(() => place([factory], [{ name: 'firmware.bin', data: bytes(0x101) }]), /fit/);
	assert.throws(() => place(TABLE, [{ name: 'notes.txt', data: bytes(1) }]), /no app image/);
});

test('firstFlashFiles writes the extras at their offsets and places the rest with their table', () => {
	const firstFlash = extras('bootloader.bin@0x1000,partitions.bin@0x8000');
	assert.deepEqual(firstFlash, [
		{ name: 'bootloader.bin', address: 0x1000 },
		{ name: 'partitions.bin', address: 0x8000 }
	]);
	const downloaded = [
		{ name: 'bootloader.bin', data: bytes(16) },
		{ name: 'partitions.bin', data: image(TABLE) },
		{ name: 'firmware.bin', data: bytes(10) }
	];
	const { files, table } = firstFlashFiles(firstFlash, downloaded, 'v1.0.0');
	assert.deepEqual(table, TABLE);
	assert.deepEqual(
		files.map((f) => [f.name, f.address]),
		[
			['bootloader.bin', 0x1000],
			['partitions.bin', 0x8000],
			['firmware.bin', 0x10000],
			['otadata', 0xe000]
		]
	);
	assert.throws(() => firstFlashFiles(firstFlash, downloaded.slice(1), 'v1.0.0'), /bootloader/);
	const noTable = extras('bootloader.bin@0x1000');
	assert.throws(
		() => firstFlashFiles(noTable, [downloaded[0], downloaded[2]], 'v1.0.0'),
		/partition table/
	);
});

/** Both otadata sectors, with an entry of sequence `seq` in each sector given (0: none). */
function otadata(...seqs) {
	const data = new Uint8Array(0x2000).fill(0xff);
	const view = new DataView(data.buffer);
	seqs.forEach((seq, sector) => {
		if (!seq) return;
		view.setUint32(sector * 0x1000, seq, true);
		view.setUint32(sector * 0x1000 + 28, otaCrc(seq), true);
	});
	return data;
}

test('otaCrc matches what ESP-IDF writes', () => {
	// otadata as esp_ota_set_boot_partition leaves it for ota_0: 01 00 00 00 … 9a 98 43 47.
	assert.equal(otaCrc(1), 0x4743989a);
});

test('bootApp follows the highest valid otadata entry', () => {
	assert.equal(bootApp(TABLE, otadata()).label, 'app0');
	assert.equal(bootApp(TABLE, otadata(1, 2)).label, 'app1');
	assert.equal(bootApp(TABLE, otadata(3, 2)).label, 'app0');
	const corrupt = otadata(1, 2);
	corrupt[0x1000 + 28] ^= 1; // a bad CRC: the bootloader ignores that entry
	assert.equal(bootApp(TABLE, corrupt).label, 'app0');
});

test('place with otadata writes the slot not running and an entry selecting it', () => {
	const { files, app } = place(TABLE, [{ name: 'firmware.bin', data: bytes(10) }], otadata(1));
	assert.equal(app.label, 'app1');
	assert.deepEqual(
		files.map((f) => [f.name, f.address]),
		[
			['firmware.bin', 0x150000],
			['otadata', 0xf000]
		]
	);
	// The board's otadata once written: sector 0 kept, sector 1 the new entry.
	const after = otadata(1);
	after.set(files[1].data, 0x1000);
	assert.equal(bootApp(TABLE, after).label, 'app1');
	// Running app1 (seq 2 in sector 1): back to app0, written over sector 0.
	const back = place(TABLE, [{ name: 'firmware.bin', data: bytes(10) }], otadata(0, 2));
	assert.equal(back.app.label, 'app0');
	assert.equal(back.files[1].address, 0xe000);
	assert.equal(new DataView(back.files[1].data.buffer).getUint32(0, true), 3);
});

test('imageChip reads the chip of app and bootloader images only', () => {
	const app = bytes(24);
	app[0] = 0xe9;
	app[12] = 9;
	assert.equal(imageChip({ name: 'firmware.bin', data: app }), 9);
	assert.equal(imageChip({ name: 'firmware-littlefs.bin', data: app }), undefined);
	assert.equal(imageChip({ name: 'firmware.bin', data: bytes(24) }), undefined);
});

test('place refuses a table pointing into the bootloader', () => {
	const bad = [{ type: 0, subtype: 0x10, offset: 0x1000, size: 0x100, label: 'app0' }];
	assert.throws(() => place(bad, [{ name: 'firmware.bin', data: bytes(1) }]), /bootloader/);
});

test('md5 matches node', () => {
	for (const n of [0, 1, 55, 56, 64, 1000, 70000]) {
		const data = Uint8Array.from({ length: n }, (_, i) => (i * 7) & 0xff);
		assert.equal(
			Buffer.from(md5(data)).toString('hex'),
			createHash('md5').update(data).digest('hex')
		);
	}
});

test('tableIntact checks the MD5 entry', () => {
	const table = image(TABLE);
	assert.ok(tableIntact(table));
	table[4] ^= 1; // an offset changed under the MD5
	assert.ok(!tableIntact(table));
	assert.ok(tableIntact(new Uint8Array(0x1000).fill(0xff))); // blank flash: no table, nothing to fail
});

/** An ESP image as esptool's elf2image writes it: one segment, checksum, SHA-256 appended. */
function app({ chip = 9, desc = true } = {}) {
	const segment = new Uint8Array(0x100);
	if (desc) {
		new DataView(segment.buffer).setUint32(0, 0xabcd5432, true);
		segment.set(new TextEncoder().encode('v1.2.0'), 16);
		segment.set(new TextEncoder().encode('ab98'), 48);
		segment.fill(0xab, 0xb0 - 32, 0xd0 - 32);
	}
	const at = 24 + 8 + segment.length;
	const padded = (at + 16) & ~15;
	const data = new Uint8Array(padded + 32);
	data.set([0xe9, 1], 0);
	data[12] = chip;
	data[23] = 1;
	new DataView(data.buffer).setUint32(24 + 4, segment.length, true);
	data.set(segment, 32);
	data[padded - 1] = segment.reduce((sum, b) => sum ^ b, 0xef);
	data.set(createHash('sha256').update(data.subarray(0, padded)).digest(), padded);
	return data;
}

test('checkImage accepts a good image and names what is wrong with a bad one', async () => {
	const good = app();
	assert.equal(await checkImage('firmware.bin', good), good.length);
	await assert.rejects(checkImage('firmware.bin', bytes(100)), /header/);
	const flipped = good.slice();
	flipped[100] ^= 1;
	await assert.rejects(checkImage('firmware.bin', flipped), /checksum/);
	await assert.rejects(checkImage('firmware.bin', good.slice(0, -1)), /bytes/);
	const padded = new Uint8Array(good.length + 16);
	padded.set(good);
	await assert.rejects(checkImage('firmware.bin', padded), /bytes/);
	assert.equal(await checkImage('in a backup', padded, false), good.length);
	const sha = good.slice();
	sha[sha.length - 1] ^= 1;
	await assert.rejects(checkImage('firmware.bin', sha), /SHA-256/);
});

test('checkFiles checks apps and tables, passes filesystem images', async () => {
	await checkFiles([
		{ name: 'firmware.bin', data: app() },
		{ name: 'partitions.bin', data: image(TABLE) },
		{ name: 'firmware-littlefs.bin', data: bytes(10) },
		{ name: 'boot_app0.bin', data: new Uint8Array(0x2000).fill(0xff) }
	]);
	await assert.rejects(checkFiles([{ name: 'firmware.bin', data: bytes(10) }]), /firmware.bin/);
	const table = image(TABLE);
	table[4] ^= 1;
	await assert.rejects(checkFiles([{ name: 'partitions.bin', data: table }]), /MD5/);
});

test('appDesc reads what an app says it is', () => {
	assert.deepEqual(appDesc(app().subarray(0, 0x100)), {
		version: 'v1.2.0',
		project: 'ab98',
		built: ' ',
		sha: 'ab'.repeat(32)
	});
	assert.equal(appDesc(app({ desc: false }).subarray(0, 0x100)), undefined);
	assert.equal(appDesc(new Uint8Array(0x100).fill(0xff)), undefined);
});
