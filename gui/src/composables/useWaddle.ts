import { ref, readonly } from 'vue';

export interface ChatMessage {
  id: string;
  from: string;
  body: string;
  sentAt: string;
}

export interface RosterItem {
  jid: string;
  name: string;
  group: string;
  subscription: string;
  presence: string;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  status: string;
  errorReason: string | null;
  errorCount: number;
  capabilities: string[];
}

export interface UiConfig {
  notifications: boolean;
  theme: string;
  locale: string | null;
  themeName: string;
  customThemePath: string | null;
}

export type PluginAction =
  | { action: 'install'; reference: string }
  | { action: 'uninstall'; pluginId: string }
  | { action: 'update'; pluginId: string }
  | { action: 'get'; pluginId: string };

export type UnlistenFn = () => void;

export interface EventCallback<T = unknown> {
  (event: { payload: T }): void;
}

export interface WaddleTransport {
  sendMessage(to: string, body: string): Promise<ChatMessage>;
  getRoster(): Promise<RosterItem[]>;
  setPresence(show: string, status?: string): Promise<void>;
  joinRoom(roomJid: string, nick: string): Promise<void>;
  leaveRoom(roomJid: string): Promise<void>;
  getHistory(jid: string, limit: number, before?: string): Promise<ChatMessage[]>;
  managePlugins(action: PluginAction): Promise<PluginInfo>;
  getConfig(): Promise<UiConfig>;
  listen<T>(channel: string, callback: EventCallback<T>): Promise<UnlistenFn>;
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function createTauriTransport(): Promise<WaddleTransport> {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');

  return {
    sendMessage: (to, body) => invoke<ChatMessage>('send_message', { to, body }),
    getRoster: () => invoke<RosterItem[]>('get_roster'),
    setPresence: (show, status) => invoke<void>('set_presence', { show, status }),
    joinRoom: (roomJid, nick) => invoke<void>('join_room', { roomJid, nick }),
    leaveRoom: (roomJid) => invoke<void>('leave_room', { roomJid }),
    getHistory: (jid, limit, before) => invoke<ChatMessage[]>('get_history', { jid, limit, before }),
    managePlugins: (action) => invoke<PluginInfo>('manage_plugins', { action }),
    getConfig: () => invoke<UiConfig>('get_config'),
    listen: <T>(channel: string, callback: EventCallback<T>) =>
      listen<T>(channel, (event) => callback({ payload: event.payload })),
  };
}

async function createWasmTransport(): Promise<WaddleTransport> {
  const wasmModuleName = 'waddle-wasm';
  const { WaddleCore } = (await import(/* @vite-ignore */ wasmModuleName)) as {
    WaddleCore: {
      init(): Promise<{
        send_message(to: string, body: string): Promise<ChatMessage>;
        get_roster(): Promise<RosterItem[]>;
        set_presence(show: string, status?: string): Promise<void>;
        join_room(roomJid: string, nick: string): Promise<void>;
        leave_room(roomJid: string): Promise<void>;
        get_history(jid: string, limit: number, before?: string): Promise<ChatMessage[]>;
        manage_plugins(action: PluginAction): Promise<PluginInfo>;
        get_config(): Promise<UiConfig>;
        on<T>(channel: string, callback: (payload: T) => void): () => void;
      }>;
    };
  };
  const core = await WaddleCore.init();

  return {
    sendMessage: (to, body) => core.send_message(to, body),
    getRoster: () => core.get_roster(),
    setPresence: (show, status) => core.set_presence(show, status),
    joinRoom: (roomJid, nick) => core.join_room(roomJid, nick),
    leaveRoom: (roomJid) => core.leave_room(roomJid),
    getHistory: (jid, limit, before) => core.get_history(jid, limit, before),
    managePlugins: (action) => core.manage_plugins(action),
    getConfig: () => core.get_config(),
    listen: <T>(channel: string, callback: EventCallback<T>) => {
      const unsubscribe = core.on(channel, (payload: T) => callback({ payload }));
      return Promise.resolve(unsubscribe);
    },
  };
}

let transportPromise: Promise<WaddleTransport> | null = null;
const ready = ref(false);

function getTransport(): Promise<WaddleTransport> {
  if (!transportPromise) {
    transportPromise = (isTauri() ? createTauriTransport() : createWasmTransport()).then(
      (transport) => {
        ready.value = true;
        return transport;
      },
    );
  }
  return transportPromise;
}

export function useWaddle() {
  const transport = getTransport();

  async function sendMessage(to: string, body: string): Promise<ChatMessage> {
    return (await transport).sendMessage(to, body);
  }

  async function getRoster(): Promise<RosterItem[]> {
    return (await transport).getRoster();
  }

  async function setPresence(show: string, status?: string): Promise<void> {
    return (await transport).setPresence(show, status);
  }

  async function joinRoom(roomJid: string, nick: string): Promise<void> {
    return (await transport).joinRoom(roomJid, nick);
  }

  async function leaveRoom(roomJid: string): Promise<void> {
    return (await transport).leaveRoom(roomJid);
  }

  async function getHistory(jid: string, limit: number, before?: string): Promise<ChatMessage[]> {
    return (await transport).getHistory(jid, limit, before);
  }

  async function managePlugins(action: PluginAction): Promise<PluginInfo> {
    return (await transport).managePlugins(action);
  }

  async function getConfig(): Promise<UiConfig> {
    return (await transport).getConfig();
  }

  async function listen<T>(channel: string, callback: EventCallback<T>): Promise<UnlistenFn> {
    return (await transport).listen(channel, callback);
  }

  return {
    ready: readonly(ready),
    sendMessage,
    getRoster,
    setPresence,
    joinRoom,
    leaveRoom,
    getHistory,
    managePlugins,
    getConfig,
    listen,
  };
}
