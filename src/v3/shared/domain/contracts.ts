import type { WakePort, WakeSetupState, WakeCapabilities } from "./wake";
import type { DictationMetadata } from "./historyMetadata";
import type { DiagnosticReportPort } from "./diagnosticReport";
import type { UpdatesPort } from "./updates";
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
export type DictationMode = "standard" | "live";
export interface LiveDictation {
  sessionId: string;
  revision: number;
  committedText: string;
  draftText: string;
  pendingText: string;
  insertionState: "active" | "paused_focus" | "failed" | "none";
  phase: "listening" | "draining" | "done" | "cancelled" | "error";
  lagMs: number;
  warning?: string;
}
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
  dictationMode: DictationMode;
  microphone: string;
  model: string;
  language: string;
  acceleration: string;
  dictionaryEnabled: boolean;
  dictionaryEntries: PersonalDictionaryEntry[];
  wakeEnabled: boolean;
  wakePhrase: string;
  wakeLanguage: "ru" | "en";
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
export interface PersonalDictionaryEntry {
  written: string;
  spoken: string[];
}
export type ToggleKey = {
  [K in keyof Preferences]: Preferences[K] extends boolean ? K : never;
}[keyof Preferences];
export interface Dictation {
  metadata?: DictationMetadata;
  findings?: {
    title: string;
    count: number;
    example: string;
    advice: string;
  }[];
  analysisReady?: boolean;
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
  apiKey?: string;
  hasApiKey?: boolean;
  id: string;
  name: string;
  provider: string;
  location: string;
  url: string;
  model: string;
}
export interface SpeechModel {
  custom?: boolean;
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
  | "load-error"
  | "live-paused"
  | "live-backlog"
  | "live-insertion-error";
export interface WorkspaceState {
  wakeSetup?: WakeSetupState;
  wakeCapabilities?: WakeCapabilities;
  live?: LiveDictation | null;
  commandProposal?: string;
  recordingSource?: string;
  accelerations?: { value: string; label: string }[];
  aiChecked?: boolean;
  devices?: { value: string; label: string }[];
  wakePhrases?: string[];
  wakeStatus?: string;
  serviceAddress?: string;
  serviceError?: string;
  countdown?: { remaining_ms: number; timeout_ms: number; speaking: boolean };
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
  testMicrophone(device?: string): Promise<void>;
  resetOverlay?(): Promise<void>;
  previewOverlay?(preferences: Preferences): Promise<void>;
  playSample(): Promise<void>;
  listModels?(profileId: string): Promise<string[]>;
  testConnection(profileId?: string): Promise<string>;
  saveProfile(profile: ConnectionProfile): Promise<void>;
  removeProfile(id: string): void;
}
export interface DictationPort {
  start(): void;
  finish(): Promise<void>;
  cancel(): void;
  resumeInsertion(): Promise<void>;
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
  recommend?(id: string): Promise<string>;
  clear(): void;
}
export interface CommandsPort {
  confirm?(): Promise<void>;
  dismiss?(): Promise<void>;
  saveApp(app: LaunchApp): void;
  removeApp(id: string): void;
  test(phrase: string): string;
}
export interface ServicePort {
  copyToken?(): Promise<void>;
  cancel(id: string): void;
  enqueue(): void;
  retry(id: string): void;
}
export interface Workspace {
  native?: boolean;
  wake?: WakePort;
  refresh?(): Promise<void>;
  state: WorkspaceState;
  settings: SettingsPort;
  dictation: DictationPort;
  history: HistoryPort;
  trainer: TrainerPort;
  commands: CommandsPort;
  service: ServicePort;
  report: DiagnosticReportPort;
  updates: UpdatesPort;
  copy(text: string): Promise<void>;
  scenario(value: Scenario): void;
  diagnostics(): Diagnostic[];
  resumeOnboarding(step: number): void;
  dispose(): void;
}
