import type { Meta, StoryObj } from "@storybook/vue3-vite";
import WebSocketPanel from "./WebSocketPanel.vue";
import { sampleResolvedVariables } from "../../.storybook/mocks/fixtures";
import type { RequestFile, WebSocketDraft } from "../types/nova";

const SAMPLE_REQUEST: RequestFile = {
  name: "live-updates",
  path: "/home/chris/projects/nova-fixtures/nova/collections/users/live-updates.nova",
  method: "",
  protocol: "websocket",
};

const SAMPLE_DRAFT: WebSocketDraft = {
  url: "{{base_url}}/ws/updates",
  headers: [{ name: "Authorization", value: "Bearer {{api_key}}" }],
  messages: [{ kind: "text", text: '{"type":"subscribe","channel":"orders"}' }],
};

const meta: Meta<typeof WebSocketPanel> = {
  title: "Components/WebSocketPanel",
  component: WebSocketPanel,
  args: {
    request: SAMPLE_REQUEST,
    selectedEnvironment: "local",
    projectRoot: "/home/chris/projects/nova-fixtures",
    active: true,
  },
  argTypes: {
    onDirtyChange: { action: "dirtyChange" },
    onSaved: { action: "saved" },
  },
};

export default meta;
type Story = StoryObj<typeof WebSocketPanel>;

export const Disconnected: Story = {
  parameters: {
    tauriMocks: {
      read_websocket_request: () => SAMPLE_DRAFT,
      get_resolved_variables: () => sampleResolvedVariables,
    },
  },
};

export const LoadFailed: Story = {
  parameters: {
    tauriMocks: {
      read_websocket_request: () => {
        throw new Error("failed to parse .nova file: unexpected [messages] entry");
      },
      get_resolved_variables: () => sampleResolvedVariables,
    },
  },
};
