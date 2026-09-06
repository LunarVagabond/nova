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
      // `VariableAwareInput`'s root has no size of its own by design — it
      // takes on whatever field class its caller applies (see the
      // `.var-input` comment in _base.scss). `.kv-editor__input` is the
      // real class request/param/variable rows use for exactly this, so
      // the story renders with it too rather than collapsing to nothing.
      template: `<div style="max-width: 28rem; padding: 8px; border: 1px dashed var(--color-border);">
        <VariableAwareInput
          class="kv-editor__input"
          :model-value="value"
          :resolved="args.resolved"
          :placeholder="args.placeholder"
          @update:model-value="value = $event"
        />
      </div>`,
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
