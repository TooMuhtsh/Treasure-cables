// Accès au serveur local de l'application, et types de la présentation
// calculée côté Rust (crate treasure-present). Le futur site web fournira
// les mêmes données à partir d'un rapport déposé.

export type Level = 'ok' | 'info' | 'warning' | 'problem';
export type SpeedVerdict = 'at_max' | 'below' | 'unknown' | 'problem';

export interface Fact {
  label: string;
  value: string;
}

export interface DeviceView {
  name: string;
  location: string;
  ids: string | null;
  is_hub: boolean;
  speed: string | null;
  speed_mbps: number | null;
  max: string | null;
  max_mbps: number | null;
  verdict: SpeedVerdict;
  verdict_label: string | null;
  facts: Fact[];
  children: DeviceView[];
}

export interface PlugView {
  kind: 'usb_c' | 'usb_a';
  label: string;
  capability: string | null;
  capability_mbps: number | null;
  locations: string[];
  display: string | null;
  devices: DeviceView[];
}

export interface ControllerView {
  id: string;
  name: string | null;
  ports: string[];
  internal_ports: number;
  devices: DeviceView[];
}

export interface LinkUsage {
  label: string;
  max: string | null;
  usage_percent: number | null;
  source: string;
}

export interface DisplayView {
  name: string;
  internal: boolean;
  connection: string | null;
  mode: string | null;
  native: string | null;
  below_native: boolean;
  mirrored: boolean;
  transport: string | null;
  gpu: string | null;
  link: LinkUsage | null;
}

export interface FindingView {
  code: string;
  level: Level;
  confidence: string;
  subject: string;
  title: string;
  detail: string;
}

export interface LinkView {
  duration_s: number;
  ports: {
    place: string;
    name: string | null;
    ids: string | null;
    connections: number;
    errors: number;
    detail: string[];
  }[];
  controller_errors: number;
}

export interface Presentation {
  os: string;
  tool_version: string;
  generated_at: number | null;
  summary: { level: Level; problems: number; warnings: number; label: string };
  plugs: PlugView[];
  internal: DeviceView[];
  controllers: ControllerView[];
  displays: DisplayView[];
  usb4: { name: string; role: string }[];
  typec_ports: { id: string; connected: boolean; roles: string | null; cable: string | null }[];
  findings: FindingView[];
  link_diagnostics: LinkView | null;
  notes: string[];
}

export type Outcome =
  | 'no_data'
  | 'not_recognized'
  | 'usb2_limited'
  | 'below_capability'
  | 'port_limited'
  | 'link_dropped'
  | 'ok';

export interface CableVerdict {
  outcome: Outcome;
  severity: 'info' | 'warning' | 'problem';
  title: string;
  detail: string;
  device: string | null;
  speed: string | null;
}

/** Étape de l'assistant « Tester un câble ». */
export type CableStep = 'closed' | 'intro' | 'watch' | 'verdict';
/** Étape de l'assistant « Tester avec mon téléphone ». */
export type PhoneStep = 'closed' | 'intro' | 'running' | 'verdict';

export interface ProbeProgress {
  phase: 'searching' | 'approval' | 'measuring';
  text: string;
  probe: string | null;
  /** Ce que voit la recherche du téléphone. */
  detail: string | null;
}

export type Theme = 'auto' | 'light' | 'dark';

export interface State {
  presentation: Presentation | null;
  error: string | null;
  live: boolean;
  platform: string;
  elevated: boolean;
  build: string;
  cable: { remaining_s: number; detected: string[]; listens_link_errors: boolean } | null;
  cable_results: CableVerdict[];
  deep: { elapsed_s: number; events: number } | null;
  message: { level: 'ok' | 'error' | 'elevation'; text: string } | null;
  theme: Theme;
  /** Version du rapport, augmentée à chaque analyse automatique. */
  version: number;
  /** Test avec le téléphone en cours. */
  probe: ProbeProgress | null;
}

// Le jeton arrive dans l'adresse ouverte par l'application ; on le garde
// pour la session et on le retire de la barre d'adresse.
const token: string = (() => {
  const fromUrl = new URLSearchParams(location.search).get('t');
  if (fromUrl) {
    sessionStorage.setItem('treasure-token', fromUrl);
    history.replaceState(null, '', location.pathname);
    return fromUrl;
  }
  return sessionStorage.getItem('treasure-token') ?? '';
})();

async function call(method: 'GET' | 'POST', path: string): Promise<State> {
  const response = await fetch(`./api/${path}`, { method, headers: { 'X-Token': token } });
  if (!response.ok) {
    throw new Error(`Le serveur local a répondu ${response.status}`);
  }
  return response.json();
}

export const api = {
  state: () => call('GET', 'state'),
  /** Répond au prochain branchement ou débranchement (ou après 25 s). */
  wait: (version: number) => call('GET', `wait?v=${version}`),
  refresh: () => call('POST', 'refresh'),
  exportReport: () => call('POST', 'export'),
  cableStart: () => call('POST', 'cable/start'),
  cableFinish: () => call('POST', 'cable/finish'),
  cableCancel: () => call('POST', 'cable/cancel'),
  cableClear: () => call('POST', 'cable/clear'),
  deepStart: () => call('POST', 'deep/start'),
  deepFinish: () => call('POST', 'deep/finish'),
  elevate: () => call('POST', 'elevate'),
  probeStart: () => call('POST', 'probe/start'),
  probeCancel: () => call('POST', 'probe/cancel'),
  setTheme: (theme: Theme) => call('POST', `theme/${theme}`),
  /** La fenêtre se ferme : le serveur s'arrête s'il n'a plus de nouvelles. */
  bye: () => {
    fetch('./api/bye', { method: 'POST', keepalive: true, headers: { 'X-Token': token } }).catch(() => {});
  },
};

/** Largeur relative d'une vitesse sur une échelle logarithmique (12 Mb/s à 80 Gb/s). */
export function speedRatio(mbps: number | null): number {
  if (!mbps) return 0;
  const min = Math.log10(1);
  const max = Math.log10(80_000);
  return Math.max(0.04, Math.min(1, (Math.log10(mbps) - min) / (max - min)));
}

/** Applique un thème à la page (« auto » suit le système). */
export function applyTheme(theme: Theme) {
  if (theme === 'auto') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

/** Icône et teinte d'un verdict de câble. */
export function verdictLook(v: CableVerdict): { icon: 'error' | 'warning' | 'checkCircle' | 'info'; tone: string } {
  if (v.severity === 'problem') return { icon: 'error', tone: 'problem' };
  if (v.severity === 'warning') return { icon: 'warning', tone: 'warning' };
  if (v.outcome === 'ok') return { icon: 'checkCircle', tone: 'ok' };
  return { icon: 'info', tone: 'info' };
}
