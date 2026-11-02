import type {
	Device,
	InstallProject,
	Project,
	ProjectInput,
	ProjectStats,
	Release
} from './types.ts';

/** Where the Rust server mounts the admin API (next to this app's `paths.base`). */
const API = '/admin/api';

export class ApiError extends Error {
	constructor(
		message: string,
		readonly status: number
	) {
		super(message);
	}
}

/** Calls the Rust API, with the Clerk session token when signed in; errors carry the server's message. */
async function call(url: string, init: RequestInit = {}): Promise<Response> {
	const token = await window.Clerk?.session?.getToken();
	const res = await fetch(url, {
		...init,
		headers: {
			'Content-Type': 'application/json',
			...(token ? { Authorization: `Bearer ${token}` } : {})
		}
	});
	if (!res.ok) {
		const body = await res.json().catch(() => ({}));
		throw new ApiError(body.error ?? res.statusText, res.status);
	}
	return res;
}

async function json<T>(url: string, init?: RequestInit): Promise<T> {
	const res = await call(url, init);
	return (res.status === 202 || res.status === 204 ? null : await res.json()) as T;
}

const request = <T>(path: string, init?: RequestInit) => json<T>(`${API}${path}`, init);

const send = (method: string, body?: unknown): RequestInit => ({
	method,
	body: body === undefined ? undefined : JSON.stringify(body)
});

/** Public: the page needs the publishable key before anyone signs in. */
export async function fetchConfig(): Promise<{ clerk_publishable_key: string }> {
	const res = await fetch(`${API}/config`);
	if (!res.ok) throw new ApiError('Beacon is unreachable', res.status);
	return res.json();
}

export const api = {
	projects: () => request<Project[]>('/projects'),
	createProject: (p: ProjectInput) => request<{ id: number }>('/projects', send('POST', p)),
	updateProject: (id: number, p: ProjectInput) => request<null>(`/projects/${id}`, send('PUT', p)),
	deleteProject: (id: number) => request<null>(`/projects/${id}`, send('DELETE')),
	syncProject: (id: number) => request<null>(`/projects/${id}/sync`, send('POST')),
	releases: (id: number) => request<Release[]>(`/projects/${id}/releases`),
	/** Any release file, first-flash ones included (src/admin/projects.rs `download`). */
	download: (id: number, tag: string, name: string) =>
		call(`${API}/projects/${id}/releases/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`)
			.then((res) => res.arrayBuffer())
			.then((buffer) => new Uint8Array(buffer)),
	stats: (id: number) => request<ProjectStats>(`/projects/${id}/stats`),
	devices: (id: number) => request<Device[]>(`/projects/${id}/devices`),
	createDevice: (id: number, name: string, publicKey: string) =>
		request<{ id: string }>(
			`/projects/${id}/devices`,
			send('POST', { name, public_key: publicKey })
		),
	renameDevice: (id: string, name: string) =>
		request<null>(`/devices/${id}`, send('PATCH', { name })),
	deleteDevice: (id: string) => request<null>(`/devices/${id}`, send('DELETE'))
};

/** The install page's version picker: public, no files (src/install.rs). */
export const projectReleases = (slug: string) => json<InstallProject>(`/api/v1/install/${slug}`);

/**
 * A device API request (`/api/v1/firmware/…`) made for the board, with the headers it signed over
 * USB; the server checks them as it checks the board's own requests. A public project's board sends
 * only its id, and none at all for its first `latest`, which gives it one.
 */
export async function deviceGet(
	path: string,
	headers: { device?: string; timestamp?: number; signature?: string }
): Promise<Response> {
	const res = await fetch(path, {
		headers: {
			...(headers.device ? { 'X-Beacon-Device': headers.device } : {}),
			...(headers.signature
				? {
						'X-Beacon-Timestamp': String(headers.timestamp),
						'X-Beacon-Signature': headers.signature
					}
				: {})
		}
	});
	if (!res.ok) {
		const body = await res.json().catch(() => ({}));
		throw new ApiError(body.error ?? res.statusText, res.status);
	}
	return res;
}

/**
 * Sync device: the board makes its own key pair and hands over only the public key, so the private
 * key never leaves it. Each sync gets a fresh nonce, so resending keygen can't swap the key mid-sync.
 */
export async function register(
	board: { ask(command: string, reply: string): Promise<string> },
	projectId: number,
	name: string
): Promise<string> {
	const nonce = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) =>
		b.toString(16).padStart(2, '0')
	).join('');
	const publicKey = await board.ask(`beacon keygen ${nonce}`, `beacon:pubkey ${nonce} `);
	const { id } = await api.createDevice(projectId, name, publicKey);
	await board.ask(`beacon id ${id}`, `beacon:ok ${id}`).catch(() => {
		throw new Error(`Registered ${id}, but the device did not save it: revoke it and sync again.`);
	});
	return id;
}
