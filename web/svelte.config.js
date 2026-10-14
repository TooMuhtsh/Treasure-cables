import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// Les composants Material Web (<md-…-button>) sont de vrais boutons,
// focalisables et utilisables au clavier : les avertissements
// d'accessibilité de Svelte sur leurs clics sont des faux positifs.
const materialButtonA11y = (warning) =>
  ['a11y_click_events_have_key_events', 'a11y_no_static_element_interactions'].includes(warning.code) &&
  /`<md-[a-z-]*button>`/.test(warning.message);

export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    warningFilter: (warning) => !materialButtonA11y(warning),
  },
};
