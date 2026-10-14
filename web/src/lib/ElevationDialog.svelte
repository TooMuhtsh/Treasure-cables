<script lang="ts">
  import Icon from './Icon.svelte';

  let { open, onconfirm, onclose }: { open: boolean; onconfirm: () => void; onclose: () => void } = $props();
</script>

<md-dialog {open} onclose={onclose}>
  <div slot="headline" class="headline"><Icon name="admin" /> Droits administrateur</div>
  <div slot="content">
    <p>
      Le diagnostic approfondi écoute la pile USB de Windows pendant que vous branchez et débranchez : il faut les
      droits administrateur. Treasure Cables va se relancer dans une nouvelle fenêtre, après l'invite de Windows.
    </p>
  </div>
  <div slot="actions">
    <md-text-button onclick={onclose}>Annuler</md-text-button>
    <md-filled-button onclick={onconfirm}>Relancer en administrateur</md-filled-button>
  </div>
</md-dialog>

<style>
  .headline {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  p {
    margin: 0;
    line-height: 1.5;
  }
</style>
