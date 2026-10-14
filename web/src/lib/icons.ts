// Icônes Material Symbols (Apache 2.0) embarquées en SVG : seules celles
// utilisées entrent dans l'exécutable, qui fonctionne hors ligne.
import usb from '@material-symbols/svg-400/outlined/usb.svg?raw';
import usbOff from '@material-symbols/svg-400/outlined/usb_off.svg?raw';
import cable from '@material-symbols/svg-400/outlined/cable.svg?raw';
import monitor from '@material-symbols/svg-400/outlined/monitor.svg?raw';
import refresh from '@material-symbols/svg-400/outlined/refresh.svg?raw';
import download from '@material-symbols/svg-400/outlined/download.svg?raw';
import checkCircle from '@material-symbols/svg-400/outlined/check_circle.svg?raw';
import warning from '@material-symbols/svg-400/outlined/warning.svg?raw';
import error from '@material-symbols/svg-400/outlined/error.svg?raw';
import info from '@material-symbols/svg-400/outlined/info.svg?raw';
import bolt from '@material-symbols/svg-400/outlined/bolt.svg?raw';
import close from '@material-symbols/svg-400/outlined/close.svg?raw';
import hub from '@material-symbols/svg-400/outlined/hub.svg?raw';
import memory from '@material-symbols/svg-400/outlined/memory.svg?raw';
import troubleshoot from '@material-symbols/svg-400/outlined/troubleshoot.svg?raw';
import admin from '@material-symbols/svg-400/outlined/admin_panel_settings.svg?raw';
import radar from '@material-symbols/svg-400/outlined/radar.svg?raw';
import devices from '@material-symbols/svg-400/outlined/devices.svg?raw';
import arrowDown from '@material-symbols/svg-400/outlined/keyboard_arrow_down.svg?raw';
import speed from '@material-symbols/svg-400/outlined/speed.svg?raw';
import phone from '@material-symbols/svg-400/outlined/mobile.svg?raw';
import themeAuto from '@material-symbols/svg-400/outlined/brightness_auto.svg?raw';
import themeLight from '@material-symbols/svg-400/outlined/light_mode.svg?raw';
import themeDark from '@material-symbols/svg-400/outlined/dark_mode.svg?raw';

export const icons = {
  usb,
  usbOff,
  cable,
  monitor,
  refresh,
  download,
  checkCircle,
  warning,
  error,
  info,
  bolt,
  close,
  hub,
  memory,
  troubleshoot,
  admin,
  radar,
  devices,
  arrowDown,
  speed,
  phone,
  themeAuto,
  themeLight,
  themeDark,
};

export type IconName = keyof typeof icons;
