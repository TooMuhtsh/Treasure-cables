<script lang="ts">
  import Icon from './Icon.svelte';
  import Scores from './Scores.svelte';
  import Verdict from './Verdict.svelte';
  import { verdictLook, type PhoneStep, type State } from './api';

  let {
    step,
    st,
    error,
    action,
    onaction,
    onstart,
    oncancel,
    onclose,
  }: {
    step: PhoneStep;
    st: State;
    error: string | null;
    action: string | null;
    onaction: () => void;
    onstart: () => void;
    oncancel: () => void;
    onclose: () => void;
  } = $props();

  let last = $derived(st.cable_results.at(-1));

  function closed() {
    if (step === 'running') oncancel();
    else if (step !== 'closed') onclose();
  }
</script>

<md-dialog open={step !== 'closed'} onclose={closed} class="wizard">
  <div slot="headline" class="headline"><Icon name="phone" /> Tester avec mon téléphone</div>
  <div slot="content" class="content">
    {#if step === 'intro'}
      {#if error}
        <div class="verdict problem">
          <Icon name="error" size={28} />
          <p>{error}</p>
        </div>
        {#if action === 'network_connections'}
          <md-outlined-button onclick={onaction}>Ouvrir les connexions réseau</md-outlined-button>
        {/if}
      {/if}
      <p>
        Le téléphone sert de testeur : l'ordinateur et l'application <strong>Treasure Probe</strong> font passer des données
        par le câble dans les deux sens, et le téléphone mesure la charge qu'il reçoit.
      </p>
      <ol class="steps">
        <li>Ouvrez <strong>Treasure Probe</strong> sur le téléphone.</li>
        <li>Branchez le téléphone à l'ordinateur <strong>avec le câble à tester</strong>.</li>
        <li>Activez le <strong>partage de connexion USB</strong> : l'application vous y emmène.</li>
      </ol>
      <p class="tip">
        <Icon name="cable" size={18} /> Câble USB-C : refaites le test en retournant la fiche côté téléphone. Des fils abîmés ne se
        voient parfois que dans un sens.
      </p>
    {:else if step === 'running'}
      <div class="watch">
        <md-circular-progress indeterminate></md-circular-progress>
        <div>
          {#if st.probe?.probe}<p class="detected"><Icon name="checkCircle" size={18} /> {st.probe.probe}</p>{/if}
          <p class="big">{st.probe?.text ?? 'Préparation…'}</p>
          {#if st.probe?.phase === 'searching'}
            <p class="muted">Application ouverte et partage de connexion USB activé ? Le téléphone est cherché pendant 2 minutes.</p>
            {#if st.probe.detail}<p class="muted"><Icon name="info" size={16} /> {st.probe.detail}</p>{/if}
            {#if action === 'network_connections'}
              <md-outlined-button onclick={onaction}>Ouvrir les connexions réseau</md-outlined-button>
            {/if}
          {/if}
        </div>
      </div>
    {:else if step === 'verdict' && last}
      <Verdict verdict={last} />
      <p class="tip">
        <Icon name="cable" size={18} /> Pour comparer, refaites le test avec un autre câble, ou avec le même en retournant la fiche
        USB-C.
      </p>
    {/if}

    {#if step !== 'running' && st.cable_results.length > (step === 'verdict' ? 1 : 0)}
      <h4>Câbles testés</h4>
      <ol class="results">
        {#each st.cable_results as r, i (i)}
          {@const l = verdictLook(r)}
          <li class={l.tone}>
            <Icon name={l.icon} size={18} />
            <span class="result">
              <span>Câble {i + 1}{r.device ? ` · ${r.device}` : ''}</span>
              {#if r.scores?.length}
                <Scores scores={r.scores} compact />
              {:else}
                <span class="muted">{r.title}</span>
              {/if}
            </span>
          </li>
        {/each}
      </ol>
    {/if}
  </div>
  <div slot="actions">
    {#if step === 'running'}
      <md-text-button onclick={oncancel}>Annuler</md-text-button>
    {:else}
      <md-text-button onclick={onclose}>Fermer</md-text-button>
      <md-filled-button onclick={onstart}>{step === 'verdict' ? 'Refaire le test' : 'Lancer le test'}</md-filled-button>
    {/if}
  </div>
</md-dialog>

<style>
  .wizard {
    max-width: 640px;
    width: calc(100vw - 48px);
  }
  .headline {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .content {
    display: grid;
    gap: 10px;
  }
  .content p {
    margin: 0;
    line-height: 1.45;
  }
  .steps {
    margin: 0;
    padding-left: 22px;
    display: grid;
    gap: 6px;
  }
  .big {
    font-size: 18px;
    font-weight: 500;
    color: var(--md-sys-color-on-surface);
  }
  .tip {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .watch {
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 12px 0;
  }
  .detected {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--treasure-ok);
    font-weight: 500;
  }
  .muted {
    color: var(--md-sys-color-on-surface-variant);
    font-size: 14px;
  }
  .verdict {
    display: flex;
    gap: 14px;
    padding: 16px;
    border-radius: 16px;
  }
  .verdict.ok {
    background: var(--treasure-ok-container);
    color: var(--treasure-on-ok-container);
  }
  .verdict.warning {
    background: var(--treasure-warn-container);
    color: var(--treasure-on-warn-container);
  }
  .verdict.problem {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
  .verdict.info {
    background: var(--md-sys-color-secondary-container);
    color: var(--md-sys-color-on-secondary-container);
  }
  h4 {
    margin: 8px 0 0;
    font: 500 14px/20px var(--md-ref-typeface-brand);
  }
  .results {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 6px;
    font-size: 14px;
  }
  .results li {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .result {
    display: grid;
    gap: 4px;
    color: var(--md-sys-color-on-surface);
  }
  .results li.ok {
    color: var(--treasure-ok);
  }
  .results li.warning {
    color: var(--treasure-warn);
  }
  .results li.problem {
    color: var(--md-sys-color-error);
  }
</style>
