<script lang="ts">
  import Icon from './Icon.svelte';
  import Scores from './Scores.svelte';
  import { verdictLook, type CableVerdict } from './api';

  /** Verdict d'un test de câble : notes d'abord, détail rangé et replié. */
  let { verdict }: { verdict: CableVerdict } = $props();

  let look = $derived(verdictLook(verdict));
  let scored = $derived((verdict.scores?.length ?? 0) > 0);
</script>

<article class="verdict">
  <header class={look.tone}>
    <Icon name={look.icon} size={28} />
    <h3>{verdict.title}</h3>
  </header>

  {#if scored}
    <Scores scores={verdict.scores ?? []} />
    {#if verdict.advice}
      <p class="advice"><Icon name="info" size={18} /> {verdict.advice}</p>
    {/if}
    <details>
      <summary>Détails</summary>
      {#if verdict.facts?.length}
        <dl>
          {#each verdict.facts as f, i (i)}
            <dt>{f.label}</dt>
            <dd>{f.value}</dd>
          {/each}
        </dl>
      {:else}
        <p>{verdict.detail}</p>
      {/if}
    </details>
  {:else}
    <p>{verdict.detail}</p>
  {/if}
</article>

<details class="legend">
  <summary>Comment lire ces notes</summary>
  <dl>
    <dt>Transfert</dt>
    <dd>
      <strong>Excellent</strong> : vitesse maximale de l'appareil (USB 3) · <strong>Bon</strong> : l'appareil n'est pas
      USB 3 · <strong>Moyen</strong> : USB 3 sous le maximum · <strong>Limité</strong> : bloqué en USB 2.0 ·
      <strong>Instable</strong>, <strong>Défaillant</strong> : coupures ou erreurs · <strong>Aucun</strong> : pas de données.
    </dd>
    <dt>Charge</dt>
    <dd>
      Mesurée par le téléphone : <strong>Rapide</strong> ≥ 1,5 A · <strong>Normale</strong> de 0,5 à 1,5 A ·
      <strong>Lente</strong> &lt; 0,5 A. Elle dépend aussi de la prise et du niveau de batterie : comparez les câbles dans les mêmes
      conditions.
    </dd>
    <dt>Général</dt>
    <dd>
      <strong>Bon câble</strong> : données et charge · <strong>Usage limité</strong> : charge et petits appareils ·
      <strong>Charge seulement</strong> : pas de données · <strong>À éviter</strong> : liaison défaillante.
    </dd>
  </dl>
</details>

<style>
  .verdict {
    display: grid;
    gap: 14px;
    padding: 16px;
    border-radius: 16px;
    background: var(--md-sys-color-surface-container-high);
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  header.ok {
    color: var(--cablescope-ok);
  }
  header.warning {
    color: var(--cablescope-warn);
  }
  header.problem {
    color: var(--md-sys-color-error);
  }
  h3 {
    margin: 0;
    font: 500 18px/24px var(--md-ref-typeface-brand);
    color: var(--md-sys-color-on-surface);
  }
  p {
    margin: 0;
    line-height: 1.45;
  }
  .advice {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 14px;
  }
  details summary {
    cursor: pointer;
    color: var(--md-sys-color-primary);
    font-size: 14px;
    font-weight: 500;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 10px 0 0;
    font-size: 14px;
  }
  dt {
    color: var(--md-sys-color-on-surface-variant);
  }
  dd {
    margin: 0;
  }
  .legend {
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .legend dl {
    grid-template-columns: 80px 1fr;
    font-size: 13px;
  }
  .legend strong {
    color: var(--md-sys-color-on-surface);
    font-weight: 500;
  }
</style>
