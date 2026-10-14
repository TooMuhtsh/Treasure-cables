<script lang="ts">
  import Icon from './Icon.svelte';
  import type { IconName } from './icons';
  import type { Level, Presentation } from './api';

  let { p }: { p: Presentation } = $props();

  const icon: Record<Level, IconName> = { ok: 'checkCircle', info: 'info', warning: 'warning', problem: 'error' };
</script>

{#if !p.findings.length}
  <article class="finding ok">
    <Icon name="checkCircle" size={28} />
    <div>
      <h3>Aucun problème détecté</h3>
      <p>Les appareils branchés fonctionnent à la vitesse attendue. Pour vérifier un câble précis, utilisez « Tester un câble ».</p>
    </div>
  </article>
{/if}
<div class="list">
  {#each p.findings as f, i (i)}
    <article class="finding {f.level}">
      <Icon name={icon[f.level]} size={28} />
      <div>
        <h3>{f.title}</h3>
        <p>{f.detail}</p>
        <p class="meta">{f.subject} · {f.confidence}</p>
      </div>
    </article>
  {/each}
</div>

{#if p.link_diagnostics}
  <section class="card">
    <h3><Icon name="radar" size={20} /> Diagnostic approfondi ({p.link_diagnostics.duration_s} s d'écoute)</h3>
    {#if !p.link_diagnostics.ports.length}
      <p>Aucun branchement USB détecté pendant l'écoute.</p>
    {:else}
      <table>
        <thead><tr><th>Prise</th><th>Appareil</th><th>Branchements reconnus</th><th>Erreurs de lien</th></tr></thead>
        <tbody>
          {#each p.link_diagnostics.ports as port, i (i)}
            <tr>
              <td>{port.place}</td>
              <td>
                {port.name ?? '—'}
                {#if port.ids && port.name !== port.ids}<span class="ids">{port.ids}</span>{/if}
                {#if port.detail.length}<span class="detail">{port.detail.join(' · ')}</span>{/if}
              </td>
              <td>{port.connections}</td>
              <td class:bad={port.errors > 0}>{port.errors}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    {#if p.link_diagnostics.controller_errors > 0}
      <p class="bad">
        {p.link_diagnostics.controller_errors} erreur(s) ou réinitialisation(s) d'un contrôleur USB pendant l'écoute.
      </p>
    {/if}
  </section>
{/if}

{#if p.notes.length}
  <section class="notes">
    {#each p.notes as note, i (i)}<p><Icon name="info" size={16} /> {note}</p>{/each}
  </section>
{/if}

<style>
  .list {
    display: grid;
    gap: 12px;
  }
  .finding {
    display: flex;
    gap: 14px;
    padding: 16px;
    border-radius: 16px;
    background: var(--md-sys-color-surface-container-low);
    border: 1px solid var(--md-sys-color-outline-variant);
  }
  .finding.ok {
    background: var(--cablescope-ok-container);
    color: var(--cablescope-on-ok-container);
    border-color: transparent;
    margin-bottom: 12px;
  }
  .finding.info :global(.icon) {
    color: var(--md-sys-color-primary);
  }
  .finding.warning {
    background: var(--cablescope-warn-container);
    color: var(--cablescope-on-warn-container);
    border-color: transparent;
  }
  .finding.problem {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
    border-color: transparent;
  }
  h3 {
    margin: 0 0 4px;
    font: 500 16px/22px var(--md-ref-typeface-brand);
    display: flex;
    align-items: center;
    gap: 8px;
  }
  p {
    margin: 0;
    line-height: 1.45;
  }
  .meta {
    margin-top: 6px;
    font-size: 12px;
    opacity: 0.8;
  }
  .card {
    margin-top: 16px;
    padding: 16px;
    border-radius: 16px;
    background: var(--md-sys-color-surface-container-low);
    border: 1px solid var(--md-sys-color-outline-variant);
    display: grid;
    gap: 10px;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 14px;
  }
  th,
  td {
    text-align: left;
    padding: 8px;
    border-bottom: 1px solid var(--md-sys-color-outline-variant);
  }
  th {
    color: var(--md-sys-color-on-surface-variant);
    font-weight: 500;
  }
  td.bad,
  p.bad {
    color: var(--md-sys-color-error);
    font-weight: 500;
  }
  .ids,
  .detail {
    display: block;
    font-size: 12px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .ids {
    font-family: ui-monospace, monospace;
  }
  .notes {
    margin-top: 16px;
    color: var(--md-sys-color-on-surface-variant);
    font-size: 13px;
    display: grid;
    gap: 4px;
  }
  .notes p {
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>
