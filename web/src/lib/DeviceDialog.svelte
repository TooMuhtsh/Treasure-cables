<script lang="ts">
  import Icon from './Icon.svelte';
  import type { IconName } from './icons';
  import { speedRatio } from './api';
  import { selection } from './selection.svelte';

  // Fiche complète d'un appareil : la vitesse d'abord, puis les rubriques.
  let device = $derived(selection.device);
  let identity = $derived(device?.sections.find((s) => s.title === 'Identité'));
  let maker = $derived(identity?.facts.find((f) => f.label === 'Fabricant')?.value);
</script>

<md-dialog open={device !== null} onclose={() => (selection.device = null)} class="sheet">
  {#if device}
    <div slot="headline" class="headline">
      <Icon name={device.is_hub ? 'hub' : 'usb'} size={28} />
      <div>
        <h2>{device.name}</h2>
        <p>{[maker, device.ids, device.location].filter(Boolean).join(' · ')}</p>
      </div>
    </div>
    <div slot="content" class="content">
      {#if device.speed && !device.is_hub}
        <section class="hero {device.verdict}">
          <div class="figures">
            <strong>{device.speed}</strong>
            {#if device.verdict_label}<span class="pill {device.verdict}">{device.verdict_label}</span>{/if}
          </div>
          <div class="gauge" title="Échelle logarithmique de 12 Mb/s à 80 Gb/s">
            <div class="fill {device.verdict}" style:width="{speedRatio(device.speed_mbps) * 100}%"></div>
            {#if device.max_mbps && device.max_mbps > (device.speed_mbps ?? 0)}
              <div class="max" style:left="{speedRatio(device.max_mbps) * 100}%" title="Maximum de l'appareil"></div>
            {/if}
          </div>
          {#if device.max}<p class="muted">Maximum de l'appareil : {device.max}</p>{/if}
        </section>
      {/if}

      <div class="sections">
        {#each device.sections as section (section.title)}
          <section class="card">
            <h3><Icon name={section.icon as IconName} size={18} /> {section.title}</h3>
            <dl>
              {#each section.facts as f, i (i)}
                <dt>{f.label}</dt>
                <dd>{f.value}</dd>
              {/each}
            </dl>
          </section>
        {/each}
      </div>

      {#if device.children.length}
        <section class="card">
          <h3><Icon name="hub" size={18} /> Branché sur ce hub</h3>
          <div class="children">
            {#each device.children as child (child.location)}
              <button onclick={() => (selection.device = child)}>
                <Icon name={child.is_hub ? 'hub' : 'usb'} size={18} />
                <span>{child.name}</span>
                <span class="muted">{child.speed ?? ''}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>
    <div slot="actions">
      <md-text-button onclick={() => (selection.device = null)}>Fermer</md-text-button>
    </div>
  {/if}
</md-dialog>

<style>
  .sheet {
    max-width: 780px;
    width: calc(100vw - 48px);
  }
  .headline {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  h2 {
    margin: 0;
    font: 400 22px/28px var(--md-ref-typeface-brand);
  }
  .headline p {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .content {
    display: grid;
    gap: 16px;
  }
  .hero {
    display: grid;
    gap: 8px;
    padding: 16px;
    border-radius: 16px;
    background: var(--md-sys-color-surface-container-high);
  }
  .figures {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .figures strong {
    font: 500 28px/34px var(--md-ref-typeface-brand);
  }
  .pill {
    padding: 3px 10px;
    border-radius: 999px;
    font-size: 13px;
    font-weight: 500;
    background: var(--md-sys-color-surface-container-highest);
  }
  .pill.at_max {
    background: var(--treasure-ok-container);
    color: var(--treasure-on-ok-container);
  }
  .pill.below {
    background: var(--treasure-warn-container);
    color: var(--treasure-on-warn-container);
  }
  .pill.problem {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
  .gauge {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--md-sys-color-surface-container-highest);
  }
  .fill {
    height: 100%;
    border-radius: 4px;
    background: var(--md-sys-color-primary);
  }
  .fill.at_max {
    background: var(--treasure-ok);
  }
  .fill.below {
    background: var(--treasure-warn);
  }
  .max {
    position: absolute;
    top: -4px;
    width: 2px;
    height: 16px;
    background: var(--md-sys-color-on-surface);
  }
  .muted {
    margin: 0;
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .sections {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr));
    gap: 12px;
  }
  .card {
    padding: 14px 16px;
    border-radius: 16px;
    border: 1px solid var(--md-sys-color-outline-variant);
    background: var(--md-sys-color-surface-container-low);
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    font: 500 15px/20px var(--md-ref-typeface-brand);
    color: var(--md-sys-color-primary);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 14px;
    margin: 0;
    font-size: 14px;
  }
  dt {
    color: var(--md-sys-color-on-surface-variant);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .children {
    display: grid;
    gap: 4px;
  }
  .children button {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--md-sys-color-on-surface);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .children button:hover {
    background: var(--md-sys-color-surface-container-highest);
  }
  .children button span:first-of-type {
    flex: 1;
  }
</style>
