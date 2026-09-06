import type { Meta, StoryObj } from "@storybook/vue3-vite";
import { ref } from "vue";
import CodeEditor, { type EditorLanguage } from "./CodeEditor.vue";
import { sampleResolvedVariables } from "../../.storybook/mocks/fixtures";

const meta: Meta<typeof CodeEditor> = {
  title: "Components/CodeEditor",
  component: CodeEditor,
  argTypes: {
    "onUpdate:modelValue": { action: "update:modelValue" },
  },
};

export default meta;
type Story = StoryObj<typeof CodeEditor>;

function withLocalState(initial: string, language: EditorLanguage, extra: Record<string, unknown> = {}) {
  return {
    render: (args: {
      modelValue: string;
      language?: EditorLanguage;
      readonly?: boolean;
      resolvedVariables?: unknown;
    }) => ({
      components: { CodeEditor },
      setup() {
        const value = ref(args.modelValue);
        return { args, value };
      },
      template: `<CodeEditor v-bind="args" :model-value="value" @update:model-value="value = $event" />`,
    }),
    args: { modelValue: initial, language, ...extra },
  };
}

export const Json: Story = withLocalState(
  JSON.stringify({ id: "{{user_id}}", name: "Ada Lovelace", roles: ["admin"] }, null, 2),
  "json",
  { resolvedVariables: sampleResolvedVariables },
);

export const InvalidJson: Story = withLocalState('{"id": "u_123", "name": ', "json");

export const Xml: Story = withLocalState(
  "<user>\n  <id>u_123</id>\n  <name>Ada Lovelace</name>\n</user>",
  "xml",
);

export const JavascriptScript: Story = withLocalState(
  "export function preRequest(ctx) {\n  ctx.headers['X-Trace-Id'] = crypto.randomUUID();\n}\n",
  "javascript",
);

export const ReadOnlyResponseBody: Story = withLocalState(
  JSON.stringify({ status: "ok", latency_ms: 42 }, null, 2),
  "json",
  { readonly: true },
);
