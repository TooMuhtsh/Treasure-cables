<script lang="ts">
  import { criterionLabel, type Score } from './api';

  /** Pastilles des notes ; `compact` pour la liste des câbles testés. */
  let { scores, compact = false }: { scores: Score[]; compact?: boolean } = $props();
</script>

<div class="scores" class:compact>
  {#each scores as s (s.criterion)}
    <div class="score">
      <span class="criterion">{criterionLabel[s.criterion]}</span>
      <span class="pill {s.tone}"><span class="dot"></span>{s.rating}</span>
      {#if s.value && !compact}<span class="value">{s.value}</span>{/if}
    </div>
  {/each}
</div>

<style>
  .scores {
    display: grid;
    gap: 8px;
  }
  .score {
    display: grid;
    grid-template-columns: 84px auto 1fr;
    align-items: center;
    gap: 10px;
  }
  .criterion {
    font-size: 13px;
    color: var(--md-sys-color-on-surface-variant);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    justify-self: start;
    padding: 3px 10px;
    border-radius: 999px;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
  }
  .pill.good {
    background: var(--treasure-ok-container);
    color: var(--treasure-on-ok-container);
  }
  .pill.fair {
    background: var(--treasure-warn-container);
    color: var(--treasure-on-warn-container);
  }
  .pill.bad {
    background: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
  .pill.unknown {
    background: var(--md-sys-color-surface-container-highest);
    color: var(--md-sys-color-on-surface-variant);
  }
  .value {
    font-size: 14px;
    font-variant-numeric: tabular-nums;
  }
  .compact {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
  }
  .compact .score {
    display: inline-flex;
    gap: 6px;
  }
  .compact .criterion {
    font-size: 12px;
  }
  .compact .pill {
    font-size: 12px;
    padding: 1px 8px;
  }
</style>
