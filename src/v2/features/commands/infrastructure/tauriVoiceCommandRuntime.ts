import { ipc, onCommandProposal } from "@/lib/ipc";

export function createTauriVoiceCommandRuntime() {
  return {
    getPending: () => ipc.getPendingVoiceCommand(),
    subscribe: (listener: (command: string) => void) =>
      onCommandProposal(listener),
    confirm: () => ipc.confirmVoiceCommand(),
    cancel: () => ipc.cancelVoiceCommand(),
  };
}
