import { mount } from 'svelte';

import '@fontsource/roboto/latin-400.css';
import '@fontsource/roboto/latin-500.css';
import './theme.css';
import './app.css';

// Composants Material 3 utilisés (Material Web, Google).
import '@material/web/button/filled-button.js';
import '@material/web/button/filled-tonal-button.js';
import '@material/web/button/text-button.js';
import '@material/web/iconbutton/icon-button.js';
import '@material/web/tabs/tabs.js';
import '@material/web/tabs/primary-tab.js';
import '@material/web/dialog/dialog.js';
import '@material/web/progress/linear-progress.js';
import '@material/web/progress/circular-progress.js';

import App from './App.svelte';

mount(App, { target: document.getElementById('app')! });
