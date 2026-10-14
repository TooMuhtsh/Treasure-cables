<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Presentation } from './api';

  let { p }: { p: Presentation } = $props();
</script>

{#if !p.displays.length}
  <p class="empty">Aucun écran détecté.</p>
{/if}
<div class="grid">
  {#each p.displays as d, i (i)}
    <article class="card">
      <header>
        <Icon name="monitor" size={28} />
        <div>
          <h3>{d.name}</h3>
          <p class="sub">{d.connection ?? 'liaison inconnue'}</p>
        </div>
      </header>
      <dl>
        {#if d.mode}<dt>Mode actuel</dt><dd>{d.mode}{d.mirrored ? ' · affichage dupliqué' : ''}</dd>{/if}
        {#if d.native}<dt>Mode natif</dt><dd>{d.native}{d.below_native ? ' · non utilisé' : ''}</dd>{/if}
        {#if d.transport}<dt>Sortie</dt><dd>{d.transport}</dd>{/if}
        {#if d.gpu}<dt>Carte graphique</dt><dd>{d.gpu}</dd>{/if}
      </dl>
      {#if d.link}
        <div class="link">
          <div class="link-head">
            <span>Lien DisplayPort : <strong>{d.link.label}</strong></span>
            {#if d.link.max}<span class="sub">max. {d.link.max}</span>{/if}
          </div>
          {#if d.link.usage_percent !== null}
            <md-linear-progress value={Math.min(1, d.link.usage_percent / 100)}></md-linear-progress>
            <p class="sub">Le mode actuel occupe {d.link.usage_percent} % du lien.</p>
          {/if}
        </div>
      {/if}
    </article>
  {/each}
</div>

{#if p.usb4.length}
  <section class="card usb4">
    <h3><Icon name="bolt" size={20} /> USB4 / Thunderbolt</h3>
    {#each p.usb4 as r (r.name)}
      <p>{r.name} : {r.role}</p>
    {/each}
  </section>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: 16px;
  }
  .card {
    background: var(--md-sys-color-surface-container-low);
    border: 1px solid var(--md-sys-color-outline-variant);
    border-radius: 16px;
    padding: 16px;
    display: grid;
    gap: 12px;
    align-content: start;
  }
  header {
    display: flex;
    gap: 12px;
    align-items: center;
    color: var(--md-sys-color-primary);
  }
  h3 {
    margin: 0;
    font: 500 18px/24px var(--md-ref-typeface-brand);
    color: var(--md-sys-color-on-surface);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .sub {
    margin: 0;
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  dl {
    margin: 0;
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    font-size: 14px;
  }
  dt {
    color: var(--md-sys-color-on-surface-variant);
  }
  dd {
    margin: 0;
  }
  .link {
    display: grid;
    gap: 8px;
    padding: 12px;
    border-radius: 12px;
    background: var(--md-sys-color-surface-container);
  }
  .link-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 14px;
  }
  .usb4 {
    margin-top: 16px;
  }
  .usb4 p {
    margin: 0;
  }
  .empty {
    color: var(--md-sys-color-outline);
  }
</style>
