import type { Meta, StoryObj } from "@storybook/vue3-vite";
import { ref } from "vue";
import KeyValueEditor from "./KeyValueEditor.vue";

type Row = { name: string; value: string; secret?: boolean };

const meta: Meta<typeof KeyValueEditor> = {
  title: "Components/KeyValueEditor",
  component: KeyValueEditor,
  argTypes: {
    "onUpdate:modelValue": { action: "update:modelValue" },
  },
};

export default meta;
type Story = StoryObj<typeof KeyValueEditor>;

function withLocalState(initial: Row[], mode: "headers" | "params" | "variables") {
  return {
    render: (args: { modelValue: Row[]; mode?: "headers" | "params" | "variables" }) => ({
      components: { KeyValueEditor },
      setup() {
        const value = ref(args.modelValue);
        return { args, value };
      },
      template: `<KeyValueEditor v-bind="args" :model-value="value" @update:model-value="value = $event" />`,
    }),
    args: { modelValue: initial, mode },
  };
}

export const Headers: Story = withLocalState(
  [
    { name: "Content-Type", value: "application/json" },
    { name: "Authorization", value: "Bearer {{access_token}}" },
  ],
  "headers",
);

export const QueryParams: Story = withLocalState(
  [
    { name: "page", value: "1" },
    { name: "limit", value: "25" },
  ],
  "params",
);

export const EnvironmentVariablesWithSecret: Story = withLocalState(
  [
    { name: "base_url", value: "https://api.staging.example.com" },
    { name: "api_key", value: "sk_live_9f8a7b6c5d4e", secret: true },
  ],
  "variables",
);

export const Empty: Story = withLocalState([], "headers");
