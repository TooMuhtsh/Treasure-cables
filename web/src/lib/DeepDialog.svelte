<script lang="ts">
  import Icon from './Icon.svelte';
  import type { State } from './api';

  let { st, onfinish }: { st: State; onfinish: () => void } = $props();
</script>

<md-dialog open={st.deep !== null} onclose={() => st.deep && onfinish()}>
  <div slot="headline" class="headline"><Icon name="radar" /> Diagnostic approfondi</div>
  <div slot="content" class="content">
    <ol>
      <li>Débranchez l'appareil ou le câble à tester.</li>
      <li>Rebranchez-le, plusieurs fois si besoin, en laissant quelques secondes entre deux essais.</li>
      <li>Terminez l'écoute (arrêt automatique après 2 minutes).</li>
    </ol>
    <div class="live">
      <md-circular-progress indeterminate></md-circular-progress>
      <p><strong>{st.deep?.elapsed_s ?? 0} s</strong> d'écoute · <strong>{st.deep?.events ?? 0}</strong> événement(s) USB</p>
    </div>
  </div>
  <div slot="actions">
    <md-filled-button onclick={onfinish}>Terminer l'écoute</md-filled-button>
  </div>
</md-dialog>

<style>
  .headline {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .content ol {
    margin: 0;
    padding-left: 20px;
    line-height: 1.6;
  }
  .live {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 16px;
  }
  .live p {
    margin: 0;
  }
</style>
