import type { Meta, StoryObj } from "@storybook/vue3-vite";
import RequestPanel from "./RequestPanel.vue";
import { sampleRequestDraft, sampleResolvedVariables } from "../../.storybook/mocks/fixtures";
import type { RequestFile } from "../types/nova";

const SAMPLE_REQUEST: RequestFile = {
  name: "get",
  path: "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova",
  method: "GET",
  protocol: "http",
};

const meta: Meta<typeof RequestPanel> = {
  title: "Components/RequestPanel",
  component: RequestPanel,
  args: {
    request: SAMPLE_REQUEST,
    selectedEnvironment: "local",
    projectRoot: "/home/chris/projects/nova-fixtures",
    active: true,
    environmentsVersion: 0,
  },
  argTypes: {
    onDirtyChange: { action: "dirtyChange" },
    onSaved: { action: "saved" },
  },
  parameters: {
    // A GET request with no scripts/multipart/graphql body only ever needs
    // these two commands to reach its loaded state — see `load()`'s
    // `readRequest`/`loadResolvedVariables`'s `getResolvedVariables` calls
    // in RequestPanel.vue. Sending, diffing, curl-paste, etc. aren't
    // exercised here and would need their own commands mocked.
    tauriMocks: {
      read_request: () => sampleRequestDraft,
      get_resolved_variables: () => sampleResolvedVariables,
    },
  },
};

export default meta;
type Story = StoryObj<typeof RequestPanel>;

export const Loaded: Story = {};

export const LoadFailed: Story = {
  parameters: {
    tauriMocks: {
      read_request: () => {
        throw new Error("failed to parse .nova file: unterminated [headers] section");
      },
      get_resolved_variables: () => sampleResolvedVariables,
    },
  },
};

// A "sent" state (clicking Send and seeing the response pane) needs a
// play function to actually click the button, which needs an interaction-
// testing addon that isn't installed in this experimental setup — see the
// fork's report for what's left to wire up.
