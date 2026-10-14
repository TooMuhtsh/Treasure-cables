<script lang="ts">
  import Icon from './Icon.svelte';
  import { verdictLook, type CableStep, type State } from './api';

  let {
    step,
    st,
    onstart,
    onfinish,
    oncancel,
    onagain,
    onclose,
  }: {
    step: CableStep;
    st: State;
    onstart: () => void;
    onfinish: () => void;
    oncancel: () => void;
    onagain: () => void;
    onclose: () => void;
  } = $props();

  const timeout = 20;
  let last = $derived(st.cable_results.at(-1));

  const look = verdictLook;

  function closed() {
    if (step === 'watch') oncancel();
    else if (step !== 'closed') onclose();
  }
</script>

<md-dialog open={step !== 'closed'} onclose={closed} class="wizard">
  <div slot="headline" class="headline"><Icon name="cable" /> Tester un câble</div>
  <div slot="content" class="content">
    {#if step === 'intro'}
      <p class="step">Étape 1 sur 2</p>
      <p class="big">Débranchez l'appareil qui servira au test.</p>
      <p>Téléphone, clé USB, SSD… Gardez le câble à tester sous la main.</p>
      <p class="tip">
        <Icon name="speed" size={18} /> Un appareil USB 3 (SSD, clé USB 3) permet aussi de vérifier si le câble est limité à l'USB 2.
      </p>
    {:else if step === 'watch'}
      <p class="step">Étape 2 sur 2</p>
      <p class="big">Branchez maintenant l'appareil avec le câble à tester.</p>
      <div class="watch">
        <md-circular-progress value={(st.cable?.remaining_s ?? 0) / timeout}></md-circular-progress>
        <div>
          <p><strong>{st.cable?.remaining_s ?? 0} s</strong> restantes</p>
          {#if st.cable?.detected.length}
            <p class="detected"><Icon name="checkCircle" size={18} /> Détecté : {st.cable.detected.join(', ')}</p>
          {:else}
            <p class="muted">En attente d'un appareil…</p>
          {/if}
        </div>
      </div>
      <p class="tip">
        {#if st.cable?.listens_link_errors}
          <Icon name="radar" size={18} /> Mode administrateur : les erreurs de lien sont aussi surveillées.
        {:else}
          <Icon name="info" size={18} /> Lancé en administrateur, le test surveillerait aussi les erreurs de lien.
        {/if}
      </p>
    {:else if step === 'verdict' && last}
      {@const l = look(last)}
      <div class="verdict {l.tone}">
        <Icon name={l.icon} size={36} />
        <div>
          <p class="big">{last.title}</p>
          <p>{last.detail}</p>
        </div>
      </div>
      {#if last.outcome === 'no_data'}
        <p class="muted">Si l'appareil était déjà branché à l'étape 1, recommencez le test.</p>
      {/if}
    {/if}

    {#if st.cable_results.length > (step === 'verdict' ? 1 : 0)}
      <h4>Câbles testés</h4>
      <ol class="results">
        {#each st.cable_results as r, i (i)}
          {@const l = look(r)}
          <li class={l.tone}>
            <Icon name={l.icon} size={18} />
            <span>
              Câble {i + 1} : {r.title}{r.device ? ` · ${r.device}` : ''}
              {#if r.measures}<span class="measures">{r.measures}</span>{/if}
            </span>
          </li>
        {/each}
      </ol>
    {/if}
  </div>
  <div slot="actions">
    {#if step === 'intro'}
      <md-text-button onclick={onclose}>Annuler</md-text-button>
      <md-filled-button onclick={onstart}>C'est débranché, continuer</md-filled-button>
    {:else if step === 'watch'}
      <md-text-button onclick={oncancel}>Annuler</md-text-button>
      <md-filled-tonal-button onclick={onfinish}>Terminer maintenant</md-filled-tonal-button>
    {:else}
      <md-text-button onclick={onclose}>Fermer</md-text-button>
      <md-filled-button onclick={onagain}>Tester un autre câble</md-filled-button>
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
  .step {
    color: var(--md-sys-color-primary);
    font-weight: 500;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
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
  .verdict .big {
    color: inherit;
    margin-bottom: 6px;
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
    align-items: center;
  }
  .measures {
    display: block;
    font-size: 12px;
    color: var(--md-sys-color-on-surface-variant);
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
