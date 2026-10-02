export type Phase =
  | "idle"
  | "listening"
  | "silence"
  | "transcribing"
  | "processing"
  | "done"
  | "cancelled"
  | "error";
export type Health = "ready" | "off" | "missing" | "loading" | "error";
export type Section =
  | "general"
  | "audio"
  | "activation"
  | "processing"
  | "overlay"
  | "privacy"
  | "diagnostics";
export type QuickPanel =
  | "microphone"
  | "wake"
  | "recognition"
  | "processing"
  | "hotkey"
  | "command-hotkey";
export interface Preferences {
  microphone: string;
  model: string;
  language: string;
  acceleration: string;
  wakeEnabled: boolean;
  wakePhrase: string;
  silenceMs: number;
  hotkey: string;
  commandHotkey: string;
  wakeThreshold: number;
  speechThreshold: number;
  processingEnabled: boolean;
  processingMode: "clean" | "format";
  profile: string;
  processingModel: string;
  instruction: string;
  autostart: boolean;
  insertion: string;
  overlayEnabled: boolean;
  overlayCompact: boolean;
  overlayScale: number;
  overlayOpacity: number;
  overlayPosition: string;
  historyEnabled: boolean;
  trainerEnabled: boolean;
  analyticsConsent: boolean;
  retentionDays: number;
  trainerAiEnabled: boolean;
  trainerModel: string;
  trainerProfile: string;
  trainerScope: "metrics_only" | "findings" | "original_text";
  cloudConsent: boolean;
  serviceEnabled: boolean;
  verboseLogging: boolean;
}
export type ToggleKey = {
  [K in keyof Preferences]: Preferences[K] extends boolean ? K : never;
}[keyof Preferences];
export interface Dictation {
  id: string;
  createdAt: string;
  text: string;
  original?: string;
  duration: number;
  title: string;
}
export interface LastSession {
  entry: Dictation | null;
  variant: "original" | "result";
  draft: string;
  edited: boolean;
  undo: string | null;
}
export interface ConnectionProfile {
  id: string;
  name: string;
  provider: string;
  location: string;
  url: string;
  model: string;
}
export interface SpeechModel {
  id: string;
  name: string;
  size: string;
  description: string;
  status: "installed" | "available" | "downloading";
  progress: number;
}
export interface LaunchApp {
  id: string;
  name: string;
  phrase: string;
  path: string;
}
export interface ServiceJob {
  id: string;
  name: string;
  state: "queued" | "running" | "done" | "error" | "cancelled";
  text?: string;
  error?: string;
  seconds: number;
}
export interface Diagnostic {
  id: string;
  title: string;
  health: Health;
  detail: string;
  section: Section;
}
export type Scenario =
  | "normal"
  | "empty"
  | "no-microphone"
  | "no-model"
  | "download"
  | "ai-error"
  | "save-error"
  | "service-off"
  | "queue"
  | "long-content"
  | "loading"
  | "load-error";
export interface WorkspaceState {
  preferences: Preferences;
  history: Dictation[];
  last: LastSession;
  models: SpeechModel[];
  applications: LaunchApp[];
  jobs: ServiceJob[];
  profiles: ConnectionProfile[];
  phase: Phase;
  audioLevel: number;
  elapsed: number;
  error: string;
  pending: Partial<Record<ToggleKey, boolean>>;
  scenario: Scenario;
  microphoneAvailable: boolean;
  aiAvailable: boolean;
  logs: string[];
  onboardingStep: number;
  testSignal: number;
}
export interface SettingsPort {
  save(patch: Partial<Preferences>): Promise<void>;
  toggle(key: ToggleKey, value: boolean): Promise<void>;
  downloadModel(id: string): Promise<void>;
  removeModel(id: string): void;
  testMicrophone(): Promise<void>;
  playSample(): Promise<void>;
  testConnection(profileId?: string): Promise<string>;
  saveProfile(profile: ConnectionProfile): Promise<void>;
  removeProfile(id: string): void;
}
export interface DictationPort {
  start(): void;
  finish(): Promise<void>;
  cancel(): void;
  chooseVariant(variant: LastSession["variant"]): void;
  edit(text: string): void;
  improve(): Promise<void>;
  undoImprove(): void;
}
export interface HistoryPort {
  remove(id: string): void;
  clear(): void;
}
export interface TrainerPort {
  clear(): void;
}
export interface CommandsPort {
  saveApp(app: LaunchApp): void;
  removeApp(id: string): void;
  test(phrase: string): string;
}
export interface ServicePort {
  cancel(id: string): void;
  enqueue(): void;
  retry(id: string): void;
}
export interface Workspace {
  state: WorkspaceState;
  settings: SettingsPort;
  dictation: DictationPort;
  history: HistoryPort;
  trainer: TrainerPort;
  commands: CommandsPort;
  service: ServicePort;
  copy(text: string): Promise<void>;
  scenario(value: Scenario): void;
  diagnostics(): Diagnostic[];
  resumeOnboarding(step: number): void;
  dispose(): void;
}
