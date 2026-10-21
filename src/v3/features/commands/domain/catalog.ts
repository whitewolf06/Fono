import { message as uiMessage } from "../../../shared/domain/messages";
export const supportedCommands = [
  {
    phrase: "пауза",
    get description() {
      return uiMessage("ui.37ed08ce83");
    },
    get group() {
      return uiMessage("ui.2f2bfeb36e");
    },
  },
  {
    phrase: "воспроизведение",
    get description() {
      return uiMessage("ui.37ed08ce83");
    },
    get group() {
      return uiMessage("ui.2f2bfeb36e");
    },
  },
  {
    phrase: "следующий трек",
    get description() {
      return uiMessage("ui.1d7ce4ae0d");
    },
    get group() {
      return uiMessage("ui.2f2bfeb36e");
    },
  },
  {
    phrase: "громче",
    get description() {
      return uiMessage("ui.350a7f43b6");
    },
    get group() {
      return uiMessage("ui.6a40ca9613");
    },
  },
  {
    phrase: "тише",
    get description() {
      return uiMessage("ui.57a4f36e03");
    },
    get group() {
      return uiMessage("ui.6a40ca9613");
    },
  },
  {
    phrase: "выключи звук",
    get description() {
      return uiMessage("ui.6f0b5458ad");
    },
    get group() {
      return uiMessage("ui.6a40ca9613");
    },
  },
  {
    phrase: "предыдущий трек",
    get description() {
      return uiMessage("ui.e17de748a0");
    },
    get group() {
      return uiMessage("ui.2f2bfeb36e");
    },
  },
  {
    phrase: "переключись на [название окна]",
    get description() {
      return uiMessage("ui.d91f81ae4d");
    },
    get group() {
      return uiMessage("ui.1e1bd9e6df");
    },
  },
];
