<script lang="ts">
  import DeviceItem from './DeviceItem.svelte';
  import Icon from './Icon.svelte';
  import PortFace from './PortFace.svelte';
  import type { Presentation } from './api';

  let { p }: { p: Presentation } = $props();
</script>

{#if !p.plugs.length && !p.internal.length}
  <p class="none">Aucune prise USB détectée sur cette machine.</p>
{/if}
<div class="grid">
  {#each p.plugs as plug (plug.locations.join())}
    <article class="card" class:empty={!plug.devices.length && !plug.display}>
      <header>
        <PortFace kind={plug.kind} superspeed={(plug.capability_mbps ?? 0) >= 5000} />
        <div>
          <h3>{plug.label}</h3>
          <p class="capability">{plug.capability ?? 'capacité inconnue'}</p>
        </div>
        <span class="where" title="Emplacement(s) logique(s) de la prise">{plug.locations.join(' · ')}</span>
      </header>
      {#if plug.display}
        <p class="display"><Icon name="monitor" size={18} /> écran {plug.display}</p>
      {/if}
      {#if plug.devices.length}
        <div class="devices">
          {#each plug.devices as device (device.location)}
            <DeviceItem {device} />
          {/each}
        </div>
      {:else if !plug.display}
        <p class="free">Libre</p>
      {/if}
    </article>
  {/each}
</div>

{#if p.internal.length}
  <section class="card internal">
    <h3><Icon name="memory" size={20} /> Appareils internes</h3>
    <div class="devices">
      {#each p.internal as device (device.location)}
        <DeviceItem {device} />
      {/each}
    </div>
  </section>
{/if}

{#if p.typec_ports.length}
  <section class="card internal">
    <h3><Icon name="cable" size={20} /> Ports USB-C décrits par le système</h3>
    {#each p.typec_ports as port (port.id)}
      <p>
        <strong>{port.id}</strong> : {port.connected ? 'connecté' : 'libre'}{port.roles ? ` (${port.roles})` : ''}
        {#if port.cable}<br />câble (e-marker) : {port.cable}{/if}
      </p>
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
    gap: 14px;
    align-content: start;
  }
  .card.empty {
    background: transparent;
    border-style: dashed;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  header > div {
    flex: 1;
  }
  h3 {
    margin: 0;
    font: 500 18px/24px var(--md-ref-typeface-brand);
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .capability {
    margin: 2px 0 0;
    font-size: 14px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .where {
    font-size: 12px;
    color: var(--md-sys-color-outline);
  }
  .display {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 14px;
    color: var(--md-sys-color-primary);
  }
  .devices {
    display: grid;
    gap: 14px;
  }
  .free {
    margin: 0;
    color: var(--md-sys-color-outline);
    font-style: italic;
  }
  .internal {
    margin-top: 16px;
  }
  .none {
    color: var(--md-sys-color-outline);
  }
  .internal p {
    margin: 0;
  }
</style>
