/** The shapes the Rust admin API returns (src/admin/*.rs). */

export interface Project {
	id: number;
	slug: string;
	name: string;
	emoji: string;
	repo: string;
	assets: string;
	synced_at: number | null;
	sync_error: string | null;
	releases: number;
	devices: number;
	latest: string | null;
}

export type ProjectInput = Pick<Project, 'slug' | 'name' | 'emoji' | 'repo' | 'assets'>;

export interface Asset {
	name: string;
	size: number;
	sha256: string | null;
	md5: string | null;
}

export interface Release {
	tag: string;
	prerelease: boolean;
	published_at: string;
	assets: Asset[];
}

export interface Device {
	id: string;
	name: string;
	created_at: number;
	last_seen: number | null;
	last_version: string | null;
}

/** Returned once, when a device is registered. */
export interface DeviceCredentials {
	id: string;
	secret: string;
}
