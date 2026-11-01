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
	/** Devices whose last reported version is this release. */
	devices: number;
	assets: Asset[];
}

export interface Device {
	id: string;
	name: string;
	created_at: number;
	last_seen: number | null;
	last_version: string | null;
	checks: number;
	downloads: number;
	up_to_date: boolean;
	/** Requests per day over the last 30 days, oldest first. */
	activity: number[];
}

/** One UTC day of a project's device traffic. */
export interface StatsDay {
	/** Unix day number (days since 1970-01-01). */
	day: number;
	checks: number;
	downloads: number;
	bytes: number;
	/** Devices that made any request. */
	active: number;
}

export interface ProjectStats {
	devices: number;
	active_24h: number;
	active_7d: number;
	up_to_date: number;
	latest: string | null;
	/** All time, from the device counters. */
	checks: number;
	downloads: number;
	/** Version changes devices reported in the last 30 days. */
	updates: number;
	/** Devices per channel over the last 30 days. */
	channels: Record<string, number>;
	/** Device count per last reported version ("unknown" if none yet). */
	versions: Record<string, number>;
	/** The last 30 days, oldest first. */
	days: StatsDay[];
}

/** Returned once, when a device is registered. */
export interface DeviceCredentials {
	id: string;
	secret: string;
}
