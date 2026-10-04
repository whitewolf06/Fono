export { default as VoiceWave } from "./presentation/VoiceWave.vue";
export { default as RecordControl } from "./presentation/RecordControl.vue";
export {
  liveIsActive,
  livePhase,
  liveStatus,
  assertDraftAvailable,
} from "./domain/live";
export { processDemoText } from "./infrastructure/mockText";
