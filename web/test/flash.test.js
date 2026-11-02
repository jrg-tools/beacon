// Where flash.ts writes each file: a wrong offset bricks a board. `pnpm test` (node --test).
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { extras, firstFlashFiles, partitions, place, tableFromImage } from '../src/lib/flash.ts';

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
	return data;
}

const bytes = (n) => new Uint8Array(n);

test('partitions parses the firmware reply and drops junk', () => {
	assert.deepEqual(partitions('0,16,10000,140000,app0;1,130,290000,160000,spiffs;x,y;'), [
		{ type: 0, subtype: 16, offset: 0x10000, size: 0x140000, label: 'app0' },
		{ type: 1, subtype: 130, offset: 0x290000, size: 0x160000, label: 'spiffs' }
	]);
	assert.deepEqual(partitions(''), []);
});

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
