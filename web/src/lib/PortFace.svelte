<script lang="ts">
  // Façade d'une prise, comme sur la machine : USB-C ovale, USB-A avec sa
  // languette bleue (USB 3) ou noire (USB 2).
  let { kind, superspeed }: { kind: 'usb_c' | 'usb_a'; superspeed: boolean } = $props();
</script>

{#if kind === 'usb_c'}
  <svg viewBox="0 0 64 40" class="face" role="img" aria-label="Prise USB-C">
    <rect x="6" y="10" width="52" height="20" rx="10" class="shell" />
    <rect x="12" y="15" width="40" height="10" rx="5" class="hole" />
    <rect x="18" y="18" width="28" height="4" rx="2" class="tongue usb3" />
  </svg>
{:else}
  <svg viewBox="0 0 64 40" class="face" role="img" aria-label={superspeed ? 'Prise USB-A (USB 3)' : 'Prise USB-A (USB 2)'}>
    <rect x="4" y="8" width="56" height="24" rx="2" class="shell" />
    <rect x="8" y="12" width="48" height="16" class="hole" />
    <rect x="8" y="12" width="48" height="7" class="tongue {superspeed ? 'usb3' : 'usb2'}" />
  </svg>
{/if}

<style>
  .face {
    width: 64px;
    height: 40px;
    flex: none;
  }
  .shell {
    fill: var(--md-sys-color-outline);
  }
  .hole {
    fill: var(--md-sys-color-inverse-surface);
  }
  .tongue.usb3 {
    fill: #005eb8; /* le vrai bleu USB 3.0 */
  }
  .tongue.usb2 {
    fill: #1b1b1f;
    stroke: var(--md-sys-color-outline-variant);
    stroke-width: 0.6;
  }
</style>
