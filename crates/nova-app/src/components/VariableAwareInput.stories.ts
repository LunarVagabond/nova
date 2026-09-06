import type { Meta, StoryObj } from "@storybook/vue3-vite";
import { ref } from "vue";
import VariableAwareInput from "./VariableAwareInput.vue";
import type { ResolvedVariables } from "../types/nova";

const RESOLVED: ResolvedVariables = {
  variables: {
    base_url: "https://api.staging.example.com",
    access_token: "sk_live_9f8a7b6c5d4e",
  },
  secrets: ["access_token"],
};

const meta: Meta<typeof VariableAwareInput> = {
  title: "Components/VariableAwareInput",
  component: VariableAwareInput,
  argTypes: {
    "onUpdate:modelValue": { action: "update:modelValue" },
  },
};

export default meta;
type Story = StoryObj<typeof VariableAwareInput>;

function withLocalState(initial: string, resolved: ResolvedVariables | null = null) {
  return {
    render: (args: { modelValue: string; resolved?: ResolvedVariables | null; placeholder?: string }) => ({
      components: { VariableAwareInput },
      setup() {
        const value = ref(args.modelValue);
        return { args, value };
      },
      template: `<VariableAwareInput :model-value="value" :resolved="args.resolved" :placeholder="args.placeholder" @update:model-value="value = $event" />`,
    }),
    args: { modelValue: initial, resolved },
  };
}

export const PlainText: Story = withLocalState("https://example.com/users");

export const WithResolvedVariables: Story = withLocalState(
  "{{base_url}}/users?token={{access_token}}",
  RESOLVED,
);

export const UnresolvedVariable: Story = withLocalState("{{undeclared_var}}/ping", RESOLVED);

export const Empty: Story = {
  ...withLocalState(""),
  args: { modelValue: "", resolved: null, placeholder: "https://example.com" },
};
