<script lang="ts">
  import { onMount } from 'svelte';
  import CableWizard from './lib/CableWizard.svelte';
  import DeepDialog from './lib/DeepDialog.svelte';
  import Details from './lib/Details.svelte';
  import Diagnosis from './lib/Diagnosis.svelte';
  import Displays from './lib/Displays.svelte';
  import ElevationDialog from './lib/ElevationDialog.svelte';
  import Icon from './lib/Icon.svelte';
  import Plugs from './lib/Plugs.svelte';
  import type { IconName } from './lib/icons';
  import { api, applyTheme, type CableStep, type State, type Theme } from './lib/api';

  let st = $state<State | null>(null);
  let tab = $state(0);
  let busy = $state(false);
  let cableStep = $state<CableStep>('closed');
  let elevationAsked = $state(false);
  let handedOff = $state(false);
  let snack = $state<{ text: string; tone: string } | null>(null);
  let snackTimer: ReturnType<typeof setTimeout> | undefined;

  const tabs: { label: string; icon: IconName }[] = [
    { label: 'Prises', icon: 'usb' },
    { label: 'Écrans', icon: 'monitor' },
    { label: 'Diagnostic', icon: 'troubleshoot' },
    { label: 'Détails', icon: 'devices' },
  ];

  // Ordre du bouton de thème, et ce qu'il annonce.
  const themes: Record<Theme, { next: Theme; icon: IconName; name: string }> = {
    auto: { next: 'light', icon: 'themeAuto', name: 'thème automatique' },
    light: { next: 'dark', icon: 'themeLight', name: 'thème clair' },
    dark: { next: 'auto', icon: 'themeDark', name: 'thème sombre' },
  };
  function themeLabel(t: Theme) {
    const name = themes[t].name.charAt(0).toUpperCase() + themes[t].name.slice(1);
    return t === 'auto' ? `${name} (suit ${st?.platform === 'windows' ? 'Windows' : 'le système'})` : name;
  }

  let p = $derived(st?.presentation ?? null);
  let theme = $derived<Theme>(st?.theme ?? 'auto');
  $effect(() => applyTheme(theme));

  async function cycleTheme() {
    const next = themes[theme].next;
    if (st) st = { ...st, theme: next };
    try {
      apply(await api.setTheme(next));
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), 'error');
    }
  }
  let alerts = $derived(p ? p.summary.problems + p.summary.warnings : 0);

  function notify(text: string, tone = 'info') {
    snack = { text: text.charAt(0).toUpperCase() + text.slice(1), tone };
    clearTimeout(snackTimer);
    snackTimer = setTimeout(() => (snack = null), 6000);
  }

  /** Applique une réponse du serveur, et ses éventuels messages. */
  function apply(next: State) {
    st = next;
    if (next.message?.level === 'elevation') elevationAsked = true;
    else if (next.message?.level === 'ok' && next.message.text.includes('administrateur')) handedOff = true;
    else if (next.message) notify(next.message.text, next.message.level === 'error' ? 'error' : 'ok');
    // Le test s'est terminé côté serveur (appareil détecté ou délai écoulé).
    if (cableStep === 'watch' && !next.cable) cableStep = 'verdict';
  }

  async function run(action: () => Promise<State>, label?: string) {
    busy = true;
    try {
      apply(await action());
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      busy = false;
      if (label) notify(label, 'ok');
    }
  }

  // Interrogation rapide pendant un test ou une écoute, lente sinon (signe
  // de vie pour le serveur, qui s'arrête quand la fenêtre est fermée).
  onMount(() => {
    run(api.state);
    let timer: ReturnType<typeof setTimeout>;
    const loop = async () => {
      const active = cableStep === 'watch' || st?.deep;
      if (!handedOff) {
        try {
          apply(await api.state());
        } catch {
          /* serveur arrêté : la fenêtre peut être fermée */
        }
      }
      timer = setTimeout(loop, active ? 1000 : 30000);
    };
    timer = setTimeout(loop, 1000);

    // Branchements et débranchements : le serveur répond à cette attente
    // dès qu'il a refait l'analyse.
    let stopped = false;
    const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    const watch = async () => {
      while (!stopped) {
        if (!st?.live || handedOff) {
          await pause(2000);
          continue;
        }
        try {
          apply(await api.wait(st.version));
        } catch {
          await pause(3000); // serveur arrêté ou occupé : on réessaie
        }
      }
    };
    watch();

    const bye = () => api.bye();
    addEventListener('pagehide', bye);
    return () => {
      stopped = true;
      clearTimeout(timer);
      removeEventListener('pagehide', bye);
    };
  });

  async function startCable() {
    busy = true;
    try {
      const next = await api.cableStart();
      apply(next);
      if (next.cable) cableStep = 'watch';
    } finally {
      busy = false;
    }
  }

  async function finishDeep() {
    await run(api.deepFinish);
    tab = 2;
  }
</script>

{#if handedOff}
  <main class="handoff">
    <Icon name="admin" size={48} />
    <h1>Treasure Cables continue dans la fenêtre administrateur</h1>
    <p>Vous pouvez fermer celle-ci.</p>
  </main>
{:else}
  <header class="appbar">
    <div class="brand">
      <span class="logo"><Icon name="usb" size={26} /></span>
      <div>
        <h1>Treasure Cables</h1>
        <p>{p?.os ?? 'Analyse en cours…'}</p>
      </div>
    </div>
    {#if p}
      <button class="summary {p.summary.level}" onclick={() => (tab = 2)}>
        <Icon name={p.summary.level === 'ok' ? 'checkCircle' : p.summary.level === 'problem' ? 'error' : 'warning'} size={20} />
        {p.summary.label}
      </button>
    {/if}
    <div class="tools">
      {#if st?.live}
        <md-icon-button title="Rafraîchir l'analyse" aria-label="Rafraîchir" disabled={busy} onclick={() => run(api.refresh, 'Analyse mise à jour')}>
          <Icon name="refresh" />
        </md-icon-button>
      {/if}
      <md-icon-button
        title="{themeLabel(theme)} — cliquer pour passer au {themes[themes[theme].next].name}"
        aria-label="{themeLabel(theme)}. Changer de thème"
        onclick={cycleTheme}
      >
        <Icon name={themes[theme].icon} />
      </md-icon-button>
      <md-icon-button title="Exporter le rapport (JSON)" aria-label="Exporter" onclick={() => run(api.exportReport)}>
        <Icon name="download" />
      </md-icon-button>
    </div>
    {#if busy}<md-linear-progress indeterminate class="busy"></md-linear-progress>{/if}
  </header>

  <main>
    {#if st && !st.live}
      <p class="banner"><Icon name="info" size={18} /> Rapport chargé depuis un fichier : le test de câble et le diagnostic approfondi se font sur la machine analysée.</p>
    {:else if st}
      <section class="actions">
        <article class="action primary">
          <Icon name="cable" size={32} />
          <div>
            <h2>Tester un câble</h2>
            <p>Branchez un appareil avec le câble : charge seule, limité à l'USB 2.0, défaillant ou correct ?</p>
          </div>
          <md-filled-button onclick={() => (cableStep = 'intro')}>Commencer</md-filled-button>
        </article>
        <article class="action">
          <Icon name="radar" size={32} />
          <div>
            <h2>Diagnostic approfondi</h2>
            <p>Écoute la pile USB pendant que vous branchez et débranchez : la preuve d'un lien instable.{st.platform === 'windows' && !st.elevated ? ' Droits administrateur requis.' : ''}</p>
          </div>
          <md-filled-tonal-button onclick={() => run(api.deepStart)}>Lancer</md-filled-tonal-button>
        </article>
      </section>
    {/if}

    {#if st?.error}
      <p class="banner error"><Icon name="error" size={18} /> {st.error}</p>
    {/if}

    {#if p}
      <md-tabs activeTabIndex={tab} onchange={(e: Event) => (tab = (e.target as HTMLElement & { activeTabIndex: number }).activeTabIndex)}>
        {#each tabs as t, i (t.label)}
          <md-primary-tab>
            <span slot="icon"><Icon name={t.icon} size={22} /></span>
            {t.label}{i === 2 && alerts ? ` (${alerts})` : ''}
          </md-primary-tab>
        {/each}
      </md-tabs>
      <section class="panel">
        {#if tab === 0}<Plugs {p} />{/if}
        {#if tab === 1}<Displays {p} />{/if}
        {#if tab === 2}<Diagnosis {p} />{/if}
        {#if tab === 3}<Details {p} />{/if}
      </section>
    {:else if !st}
      <div class="loading"><md-circular-progress indeterminate></md-circular-progress></div>
    {/if}
  </main>

  <footer>Treasure Cables {p?.tool_version ?? ''} · {st?.build ?? ''}</footer>

  {#if st}
    <CableWizard
      step={cableStep}
      {st}
      onstart={startCable}
      onfinish={async () => {
        await run(api.cableFinish);
        cableStep = 'verdict';
      }}
      oncancel={async () => {
        cableStep = 'closed';
        await run(api.cableCancel);
      }}
      onagain={() => (cableStep = 'intro')}
      onclose={() => (cableStep = 'closed')}
    />
    <DeepDialog {st} onfinish={finishDeep} />
    <ElevationDialog
      open={elevationAsked}
      onconfirm={() => {
        elevationAsked = false;
        run(api.elevate);
      }}
      onclose={() => (elevationAsked = false)}
    />
  {/if}

  {#if snack}
    <div class="snackbar {snack.tone}" role="status">{snack.text}</div>
  {/if}
{/if}

<style>
  .appbar {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 24px;
    background: var(--md-sys-color-surface-container);
    color: var(--md-sys-color-on-surface);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 1;
    min-width: 0;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 12px;
    background: var(--md-sys-color-primary-container);
    color: var(--md-sys-color-on-primary-container);
  }
  h1 {
    margin: 0;
    font: 500 22px/28px var(--md-ref-typeface-brand);
  }
  .brand p {
    margin: 0;
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .summary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border: none;
    border-radius: 8px;
    font: 500 14px/20px var(--md-ref-typeface-plain);
    cursor: pointer;
  }
  .summary.ok {
    background: var(--treasure-ok-container);
    color: var(--treasure-on-ok-container);
  }
  .summary.warning {
    background: var(--treasure-warn-container);
    color: var(--treasure-on-warn-container);
  }
  .summary.problem {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
  .tools {
    display: flex;
    gap: 4px;
  }
  .busy {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
  }
  main {
    max-width: 1180px;
    margin: 0 auto;
    padding: 20px 24px 32px;
  }
  .actions {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
    gap: 16px;
    margin-bottom: 20px;
  }
  .action {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 20px;
    border-radius: 24px;
    background: var(--md-sys-color-surface-container-high);
    color: var(--md-sys-color-on-surface);
  }
  .action.primary {
    background: var(--md-sys-color-primary-container);
    color: var(--md-sys-color-on-primary-container);
  }
  .action > div {
    flex: 1;
  }
  .action h2 {
    margin: 0 0 4px;
    font: 500 18px/24px var(--md-ref-typeface-brand);
  }
  .action p {
    margin: 0;
    font-size: 14px;
    line-height: 1.4;
    opacity: 0.9;
  }
  .action.primary md-filled-button {
    --md-filled-button-container-color: var(--md-sys-color-on-primary-container);
    --md-filled-button-label-text-color: var(--md-sys-color-primary-container);
  }
  md-tabs {
    background: transparent;
    margin-bottom: 16px;
  }
  .panel {
    min-height: 200px;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 16px;
    padding: 12px 16px;
    border-radius: 12px;
    background: var(--md-sys-color-secondary-container);
    color: var(--md-sys-color-on-secondary-container);
  }
  .banner.error {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
  .loading {
    display: grid;
    place-items: center;
    padding: 80px;
  }
  footer {
    text-align: center;
    padding: 16px;
    font-size: 12px;
    color: var(--md-sys-color-outline);
  }
  .snackbar {
    position: fixed;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    max-width: min(640px, calc(100vw - 48px));
    padding: 14px 18px;
    border-radius: 8px;
    background: var(--md-sys-color-inverse-surface);
    color: var(--md-sys-color-inverse-on-surface);
    box-shadow: 0 3px 8px rgb(0 0 0 / 0.3);
    z-index: 10;
  }
  .snackbar.error {
    background: var(--md-sys-color-error);
    color: var(--md-sys-color-on-error);
  }
  .handoff {
    display: grid;
    place-items: center;
    text-align: center;
    min-height: 80vh;
    color: var(--md-sys-color-primary);
  }
  .handoff h1 {
    color: var(--md-sys-color-on-surface);
  }
</style>
