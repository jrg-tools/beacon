/** "5m ago" from unix seconds. */
export function ago(unix: number | null): string {
	if (!unix) return 'never';
	const s = Math.max(0, Math.round(Date.now() / 1000 - unix));
	if (s < 60) return `${s}s ago`;
	if (s < 3600) return `${Math.round(s / 60)}m ago`;
	if (s < 86400) return `${Math.round(s / 3600)}h ago`;
	return `${Math.round(s / 86400)}d ago`;
}

export function size(bytes: number): string {
	return bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

export function date(iso: string): string {
	return iso ? new Date(iso).toLocaleString() : '—';
}
