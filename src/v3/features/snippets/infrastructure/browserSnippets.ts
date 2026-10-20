import {
  parseSnippetJson,
  snippetJson,
  snippetLimits,
  type SnippetTransferPort,
} from "../domain/snippets";
export function createBrowserSnippetTransfer(): SnippetTransferPort {
  return {
    import: () =>
      new Promise((resolve, reject) => {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = ".json,application/json";
        input.addEventListener("cancel", () => resolve(null), { once: true });
        input.addEventListener(
          "change",
          () => {
            const file = input.files?.[0];
            if (!file) {
              resolve(null);
              return;
            }
            if (file.size > snippetLimits.jsonBytes) {
              reject(new Error("JSON-файл превышает 1 МиБ."));
              return;
            }
            void file
              .text()
              .then((text) => resolve(parseSnippetJson(text)))
              .catch(reject);
          },
          { once: true },
        );
        input.click();
      }),
    async export(entries) {
      const url = URL.createObjectURL(
        new Blob([snippetJson(entries)], {
          type: "application/json;charset=utf-8",
        }),
      );
      const link = document.createElement("a");
      link.href = url;
      link.download = "fono-snippets.json";
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      return true;
    },
  };
}
