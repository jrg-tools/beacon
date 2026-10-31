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
			// Static files served by the Rust server under /admin; it owns every API and the config.
			adapter: adapter({ fallback: 'index.html' }),
			paths: { base: '/admin' }
		})
	],
	// `pnpm dev` talks to a local `cargo run`.
	server: { proxy: { '/admin/api': 'http://127.0.0.1:8080' } }
});
