export type FileExportFormat = "txt" | "srt" | "vtt";
export interface AddFilesResult {
  accepted: string[];
  rejected: number;
}
export interface FilesPort {
  choose(): Promise<AddFilesResult>;
  cancel(id: string): Promise<void>;
  export(id: string, format: FileExportFormat): Promise<boolean>;
  bindDrop(handlers: {
    hover(active: boolean): void;
    added(result: AddFilesResult): void;
    error(error: unknown): void;
  }): Promise<() => void>;
}
