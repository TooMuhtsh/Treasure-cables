// Appareil dont la fiche est ouverte (clic sur un appareil, n'importe où).
import type { DeviceView } from './api';

export const selection = $state<{ device: DeviceView | null }>({ device: null });
