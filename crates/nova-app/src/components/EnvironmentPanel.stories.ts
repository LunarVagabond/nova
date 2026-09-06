import type { Meta, StoryObj } from "@storybook/vue3-vite";
import EnvironmentPanel from "./EnvironmentPanel.vue";
import type { NovaEnvironment } from "../types/nova";

const SAMPLE_ENV: NovaEnvironment = {
  name: "staging",
  variables: {
    base_url: "https://api.staging.example.com",
    api_key: "sk_live_9f8a7b6c5d4e",
  },
  secrets: ["api_key"],
  auth: { type: "bearer", token: "{{api_key}}" },
  path: "/home/chris/projects/nova-fixtures/nova/envs/staging.yaml",
};

const meta: Meta<typeof EnvironmentPanel> = {
  title: "Components/EnvironmentPanel",
  component: EnvironmentPanel,
  args: {
    projectRoot: "/home/chris/projects/nova-fixtures",
  },
  argTypes: {
    onDirtyChange: { action: "dirtyChange" },
    onSaved: { action: "saved" },
    onDelete: { action: "delete" },
  },
};

export default meta;
type Story = StoryObj<typeof EnvironmentPanel>;

export const Populated: Story = {
  args: { environment: SAMPLE_ENV },
};

export const NewEmptyEnvironment: Story = {
  args: {
    environment: {
      name: "untitled",
      variables: {},
      secrets: [],
      auth: null,
      path: "/home/chris/projects/nova-fixtures/nova/envs/untitled.yaml",
    },
  },
};
