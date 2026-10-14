<script lang="ts">
  import DeviceItem from './DeviceItem.svelte';
  import Icon from './Icon.svelte';
  import { speedRatio, type DeviceView } from './api';
  import { selection } from './selection.svelte';

  let { device }: { device: DeviceView } = $props();
</script>

<div class="device">
  <button class="row" title="Voir la fiche complète" onclick={() => (selection.device = device)}>
    <Icon name={device.is_hub ? 'hub' : 'usb'} size={20} />
    <div class="name">
      <span class="title">{device.name}</span>
      {#if device.is_hub}<span class="tag">hub ou dock</span>{/if}
    </div>
    <span class="speed {device.verdict}">
      {#if device.verdict === 'problem'}
        <Icon name="error" size={16} /> problème de connexion
      {:else}
        {device.speed ?? 'vitesse inconnue'}
        {#if device.verdict === 'at_max'}<Icon name="checkCircle" size={16} />{/if}
        {#if device.verdict === 'below'}<Icon name="warning" size={16} />{/if}
      {/if}
    </span>
    <Icon name="chevronRight" size={18} />
  </button>
  {#if device.speed_mbps && !device.is_hub}
    <div class="gauge" title="Échelle logarithmique de 12 Mb/s à 80 Gb/s">
      <div class="fill {device.verdict}" style:width="{speedRatio(device.speed_mbps) * 100}%"></div>
      {#if device.max_mbps && device.max_mbps > device.speed_mbps}
        <div class="max" style:left="{speedRatio(device.max_mbps) * 100}%"></div>
      {/if}
    </div>
    {#if device.verdict === 'below'}
      <p class="hint">{device.name} {device.verdict_label} : le câble ou la prise le ralentit.</p>
    {/if}
  {/if}
  {#if device.children.length}
    <div class="children">
      {#each device.children as child (child.location)}
        <DeviceItem device={child} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .device {
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    margin: -4px -6px;
    padding: 4px 6px;
    box-sizing: content-box;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--md-sys-color-on-surface);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--md-sys-color-surface-container-highest);
  }
  .name {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .title {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    font-size: 12px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .speed {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font: 500 13px/1 var(--md-ref-typeface-plain);
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--md-sys-color-surface-container-highest);
    color: var(--md-sys-color-on-surface-variant);
    white-space: nowrap;
  }
  .speed.at_max {
    background: var(--cablescope-ok-container);
    color: var(--cablescope-on-ok-container);
  }
  .speed.below {
    background: var(--cablescope-warn-container);
    color: var(--cablescope-on-warn-container);
  }
  .speed.problem {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
  .gauge {
    position: relative;
    height: 6px;
    margin-left: 30px;
    border-radius: 3px;
    background: var(--md-sys-color-surface-container-highest);
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--md-sys-color-primary);
  }
  .fill.below {
    background: var(--cablescope-warn);
  }
  .max {
    position: absolute;
    top: -3px;
    width: 2px;
    height: 12px;
    background: var(--md-sys-color-on-surface-variant);
  }
  .hint {
    margin: 0 0 0 30px;
    font-size: 13px;
    color: var(--cablescope-on-warn-container);
  }
  .children {
    margin-left: 10px;
    padding-left: 18px;
    border-left: 2px solid var(--md-sys-color-outline-variant);
    display: grid;
    gap: 12px;
  }
</style>
