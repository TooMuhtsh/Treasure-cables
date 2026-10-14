import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Chemins relatifs : la même compilation sert la fenêtre de l'application
// (actifs embarqués dans l'exécutable) et le futur site web.
export default defineConfig({
  base: './',
  plugins: [svelte()],
  // Le logo vient de docs/logo, hors du dossier web/.
  server: { fs: { allow: ['..'] } },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    assetsInlineLimit: 0,
  },
});
