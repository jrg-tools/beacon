import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Static files the Rust server serves; it owns every API and the config. The admin pages
			// live under routes/admin and the install page at /firmware/<slug>.
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	// `pnpm dev` talks to a local `cargo run`.
	server: { proxy: { '/admin/api': 'http://127.0.0.1:8080' } }
});
