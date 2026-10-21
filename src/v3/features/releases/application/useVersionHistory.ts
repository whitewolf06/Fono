import { message as uiMessage } from "../../../shared/domain/messages";
import { computed, ref, toValue, type MaybeRefOrGetter } from "vue";
import { history05 } from "../domain/history05";
import { history06 } from "../domain/history06";
import { compareVersions, type VersionSeries } from "../domain/versionHistory";

export function useVersionHistory(currentVersion: MaybeRefOrGetter<string>) {
  const expanded = ref(false);
  const current = computed(() => toValue(currentVersion));
  const series: VersionSeries[] = [
    {
      id: "0.6",
      title: "Fono 0.6",
      get description() {
        return uiMessage("ui.b199ab64fe");
      },
      entries: [...history06].sort((a, b) =>
        compareVersions(b.version, a.version),
      ),
    },
    {
      id: "0.5",
      title: "Fono 0.5",
      get description() {
        return uiMessage("ui.a59cec317e");
      },
      entries: [...history05].sort((a, b) =>
        compareVersions(b.version, a.version),
      ),
    },
  ];
  const recent = computed(() =>
    expanded.value ? series[0].entries : series[0].entries.slice(0, 5),
  );
  const hiddenCount = computed(
    () => series[0].entries.length - recent.value.length,
  );
  return {
    current,
    expanded,
    recent,
    hiddenCount,
    latest: series[0],
    previous: series[1],
  };
}
