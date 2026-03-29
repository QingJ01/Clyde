import { writable } from 'svelte/store';

export type PetState =
  | 'idle' | 'yawning' | 'dozing' | 'collapsing'
  | 'thinking' | 'working' | 'juggling' | 'sweeping'
  | 'error' | 'attention' | 'notification' | 'carrying'
  | 'sleeping' | 'waking'
  | 'mini-idle' | 'mini-alert' | 'mini-happy'
  | 'mini-enter' | 'mini-peek' | 'mini-crabwalk'
  | 'mini-enter-sleep' | 'mini-sleep';

export const currentState = writable<PetState>('idle');
export const currentSvg   = writable<string>('clyde-idle-follow.svg');
export const dndEnabled   = writable<boolean>(false);
export const currentLang  = writable<string>('en');
