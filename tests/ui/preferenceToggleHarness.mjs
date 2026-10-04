import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as Vue from "vue";
import { createRenderer, h, nextTick, reactive } from "vue";
import { compileScript, parse } from "@vue/compiler-sfc";
import { transform } from "esbuild";
import * as WhiteUI from "@whitelife-core/ui-kit";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

export async function createToggleHarness(
  server,
  workspaceKey,
  interactionKey,
  options = {},
) {
  // Execute the real SFC client render and WhiteUI checkbox directive, without
  // adding a DOM dependency or substituting the production switch component.
  const source = readFileSync(
    options.componentPath ||
      "src/v3/features/preferences/presentation/PreferenceToggle.vue",
    "utf8",
  );
  const script = compileScript(parse(source).descriptor, {
    id: "preference-toggle-regression",
    inlineTemplate: true,
  });
  const compiled = await transform(script.content, {
    loader: "ts",
    format: "cjs",
    target: "es2022",
  });
  const modules = { vue: Vue, "@whitelife-core/ui-kit": WhiteUI };
  for (const name of ["workspace", "interaction", "feedback"])
    modules[`../../../shared/application/${name}`] = await server.ssrLoadModule(
      `/src/v3/shared/application/${name}.ts`,
    );
  modules["../../../shared/domain/wakeAvailability"] =
    await server.ssrLoadModule("/src/v3/shared/domain/wakeAvailability.ts");
  for (const [specifier, path] of Object.entries(options.imports || {}))
    modules[specifier] = await server.ssrLoadModule(path);
  const module = { exports: {} };
  new Function("require", "module", "exports", compiled.code)(
    (name) => {
      assert.ok(name in modules, `Unexpected component dependency: ${name}`);
      return modules[name];
    },
    module,
    module.exports,
  );
  const PreferenceToggle = module.exports.default;

  return (name, state, confirm) => {
    const document = { body: {}, activeElement: null };
    const element = (tag) => ({
      tag,
      tagName: tag.toUpperCase(),
      children: [],
      props: {},
      ownerDocument: document,
      isConnected: true,
      listeners: new Map(),
      addEventListener(event, listener) {
        this.listeners.set(event, [
          ...(this.listeners.get(event) || []),
          listener,
        ]);
      },
      removeEventListener(event, listener) {
        this.listeners.set(
          event,
          this.listeners.get(event)?.filter((item) => item !== listener) || [],
        );
      },
      focus() {
        document.activeElement = this;
        for (const listener of this.listeners.get("focus") || [])
          listener({ type: "focus", target: this, currentTarget: this });
      },
    });
    const renderer = createRenderer({
      createElement: element,
      createText: (text) => ({ text }),
      createComment: (text) => ({ text }),
      setText: (node, text) => (node.text = text),
      setElementText: (node, text) => (node.text = text),
      parentNode: (node) => node.parent,
      nextSibling: () => null,
      insert(node, parent, anchor) {
        node.parent = parent;
        const index = parent.children.indexOf(anchor);
        parent.children.splice(
          index < 0 ? parent.children.length : index,
          0,
          node,
        );
      },
      remove(node) {
        node.isConnected = false;
        node.parent.children = node.parent.children.filter(
          (item) => item !== node,
        );
      },
      patchProp(node, key, previous, value) {
        if (key === "disabled") {
          node.disabled = !!value;
          if (value && document.activeElement === node)
            document.activeElement = document.body;
        }
        if (/^on[A-Z]/.test(key) && !key.includes(":")) {
          const event = key.slice(2).toLowerCase();
          if (previous) node.removeEventListener(event, previous);
          if (value) node.addEventListener(event, value);
        }
        node.props[key] = value;
      },
    });
    const root = element("root");
    const workspace = {
      native: true,
      state: reactive({ pending: {}, ...state }),
      updates: state.updates,
      settings: {
        async save(patch) {
          Object.assign(workspace.state.preferences, patch);
        },
        async toggle(key, value) {
          workspace.state.preferences[key] = value;
        },
      },
    };
    const app = renderer.createApp({
      render: () =>
        h(PreferenceToggle, { name, label: "Test switch", ...options.props }),
    });
    app.use(WlConfig);
    app.use(WlToastService);
    app.provide(workspaceKey, workspace);
    app.provide(interactionKey, { confirm });
    app.mount(root);
    const findInput = (node) =>
      node.tag === "input" ? node : node.children?.map(findInput).find(Boolean);
    const input = findInput(root);
    input.focus();
    return {
      input,
      state: workspace.state,
      assertState(expected) {
        assert.equal(
          findInput(root),
          input,
          "Focus target must not be remounted",
        );
        if (!input.disabled) assert.equal(document.activeElement, input);
        assert.equal(
          input.checked,
          expected,
          "Native checked must match model",
        );
        assert.equal(input.props["aria-checked"], expected);
        assert.equal(input.parent.props.class.includes("is-checked"), expected);
      },
      async change(value) {
        input.checked = value;
        for (const listener of [...input.listeners.get("change")]) {
          listener({ type: "change", target: input, currentTarget: input });
          // Native event callbacks can have a microtask checkpoint between
          // Vue's model listener and the presentation change listener.
          await nextTick();
        }
      },
      dispose: () => app.unmount(),
    };
  };
}

export async function flushChanges() {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  await nextTick();
  await nextTick();
}
