<script lang="ts">
  import Icon from './Icon.svelte';
  import type { DeviceView, Presentation } from './api';

  let { p }: { p: Presentation } = $props();
</script>

{#snippet device(d: DeviceView)}
  <details class="device">
    <summary>
      <Icon name="arrowDown" size={18} />
      <span class="loc">{d.location}</span>
      <span class="title">{d.name}</span>
      {#if d.ids}<span class="ids">{d.ids}</span>{/if}
    </summary>
    <dl>
      {#each d.facts as fact, i (i)}
        <dt>{fact.label}</dt>
        <dd>{fact.value}</dd>
      {/each}
    </dl>
    {#if d.children.length}
      <div class="children">
        {#each d.children as child (child.location)}{@render device(child)}{/each}
      </div>
    {/if}
  </details>
{/snippet}

{#each p.controllers as c (c.id)}
  <section class="card">
    <h3><Icon name="devices" size={20} /> {c.id}{c.name ? ` · ${c.name}` : ''}</h3>
    {#if c.ports.length || c.internal_ports}
      <ul class="ports">
        {#each c.ports as port (port)}<li>{port}</li>{/each}
        {#if c.internal_ports}<li class="muted">+ {c.internal_ports} port(s) interne(s)</li>{/if}
      </ul>
    {/if}
    {#each c.devices as d (d.location)}{@render device(d)}{/each}
  </section>
{/each}

<style>
  .card {
    padding: 16px;
    margin-bottom: 16px;
    border-radius: 16px;
    background: var(--md-sys-color-surface-container-low);
    border: 1px solid var(--md-sys-color-outline-variant);
    display: grid;
    gap: 8px;
  }
  h3 {
    margin: 0;
    font: 500 16px/22px var(--md-ref-typeface-brand);
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .ports {
    margin: 0;
    padding-left: 20px;
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .muted {
    color: var(--md-sys-color-outline);
  }
  .device summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    cursor: pointer;
    list-style: none;
  }
  .device summary::-webkit-details-marker {
    display: none;
  }
  .device :global(.icon) {
    transition: transform 0.15s;
    transform: rotate(-90deg);
  }
  .device[open] > summary :global(.icon) {
    transform: none;
  }
  .loc {
    font: 500 12px/1 ui-monospace, monospace;
    color: var(--md-sys-color-primary);
    min-width: 48px;
  }
  .title {
    font-weight: 500;
  }
  .ids {
    font: 12px ui-monospace, monospace;
    color: var(--md-sys-color-outline);
  }
  dl {
    margin: 0 0 8px 26px;
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 3px 14px;
    font-size: 13px;
  }
  dt {
    color: var(--md-sys-color-on-surface-variant);
  }
  dd {
    margin: 0;
  }
  .children {
    margin-left: 12px;
    padding-left: 12px;
    border-left: 2px solid var(--md-sys-color-outline-variant);
  }
</style>
