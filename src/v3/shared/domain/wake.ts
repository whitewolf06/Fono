export type WakeValidationKind = "positive" | "silence" | "other_phrase";
export interface WakeCapabilities {
  backend: string;
  customPhrase: boolean;
  languages: { value: "ru" | "en"; label: string }[];
}
export interface WakeSetupState {
  modelReady: boolean;
  profileReady: boolean;
  verified: boolean;
  active: boolean;
  required: number;
  accepted: number;
  rejected: number;
  phrase: string;
  latest?: { accepted: boolean; reason?: string; detected?: boolean };
  validation: {
    active: boolean;
    completed: boolean;
    failed: boolean;
    positivePassed: number;
    positiveRequired: number;
    silencePassed: boolean;
    otherPhrasePassed: boolean;
  };
}
export interface WakePort {
  load(): Promise<void>;
  download(): Promise<void>;
  test(): Promise<string>;
  begin(): Promise<void>;
  record(): Promise<void>;
  beginValidation(): Promise<void>;
  validate(kind: WakeValidationKind): Promise<void>;
  cancel(): Promise<void>;
}
