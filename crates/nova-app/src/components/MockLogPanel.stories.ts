import type { Meta, StoryObj } from "@storybook/vue3-vite";
import MockLogPanel from "./MockLogPanel.vue";
import { sampleMockCallLog } from "../../.storybook/mocks/fixtures";

const meta: Meta<typeof MockLogPanel> = {
  title: "Components/MockLogPanel",
  component: MockLogPanel,
  args: {
    active: true,
  },
};

export default meta;
type Story = StoryObj<typeof MockLogPanel>;

export const ServerRunningWithTraffic: Story = {
  args: {
    mockServerStatus: { running: true, host: "127.0.0.1", port: 4010 },
  },
  parameters: {
    tauriMocks: {
      get_mock_call_log: () => sampleMockCallLog,
    },
  },
};

export const ServerRunningNoTraffic: Story = {
  args: {
    mockServerStatus: { running: true, host: "127.0.0.1", port: 4010 },
  },
  parameters: {
    tauriMocks: {
      get_mock_call_log: () => [],
    },
  },
};

export const ServerStopped: Story = {
  args: {
    mockServerStatus: { running: false, host: null, port: null },
  },
};
