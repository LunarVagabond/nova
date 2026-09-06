import type { Meta, StoryObj } from "@storybook/vue3-vite";
import EmptyState from "./EmptyState.vue";

const meta: Meta<typeof EmptyState> = {
  title: "Components/EmptyState",
  component: EmptyState,
  argTypes: {
    onOpen: { action: "open" },
    "onStart-new": { action: "start-new" },
  },
};

export default meta;
type Story = StoryObj<typeof EmptyState>;

export const Default: Story = {
  args: {},
};

export const WithError: Story = {
  args: {
    error: "That folder doesn't contain a nova/nova.yaml project — open a different folder or start a new one.",
  },
};
