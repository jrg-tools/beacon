import type {
	Device,
	DeviceCredentials,
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

/** Calls the Rust admin API with the Clerk session token; errors carry the server's message. */
async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
	const token = await window.Clerk?.session?.getToken();
	const res = await fetch(`${API}${path}`, {
		...init,
		headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` }
	});
	if (!res.ok) {
		const body = await res.json().catch(() => ({}));
		throw new ApiError(body.error ?? res.statusText, res.status);
	}
	return (res.status === 202 || res.status === 204 ? null : await res.json()) as T;
}

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
	stats: (id: number) => request<ProjectStats>(`/projects/${id}/stats`),
	devices: (id: number) => request<Device[]>(`/projects/${id}/devices`),
	createDevice: (id: number, name: string) =>
		request<DeviceCredentials>(`/projects/${id}/devices`, send('POST', { name })),
	deleteDevice: (id: string) => request<null>(`/devices/${id}`, send('DELETE'))
};
