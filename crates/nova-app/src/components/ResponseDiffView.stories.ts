import type { Meta, StoryObj } from "@storybook/vue3-vite";
import ResponseDiffView from "./ResponseDiffView.vue";
import type { ResponseDiff } from "../types/nova";

const meta: Meta<typeof ResponseDiffView> = {
  title: "Components/ResponseDiffView",
  component: ResponseDiffView,
};

export default meta;
type Story = StoryObj<typeof ResponseDiffView>;

export const Identical: Story = {
  args: {
    diff: {
      status: null,
      header_changes: [],
      body: { kind: "Unchanged" },
      identical: true,
    } satisfies ResponseDiff,
  },
};

export const StatusAndHeadersChanged: Story = {
  args: {
    diff: {
      status: { before: 200, after: 404 },
      header_changes: [
        { kind: "Added", name: "X-Request-Id", value: "a1b2c3" },
        { kind: "Removed", name: "Cache-Control", value: "no-store" },
        { kind: "Changed", name: "Content-Type", before: "application/json", after: "text/plain" },
      ],
      body: { kind: "Unchanged" },
      identical: false,
    } satisfies ResponseDiff,
  },
};

export const JsonBodyChanged: Story = {
  args: {
    diff: {
      status: null,
      header_changes: [],
      body: {
        kind: "Json",
        changes: [
          { kind: "Added", path: "$.user.verified", value: true },
          { kind: "Removed", path: "$.user.legacy_id", value: 4821 },
          { kind: "Changed", path: "$.user.email", before: "old@example.com", after: "new@example.com" },
        ],
      },
      identical: false,
    } satisfies ResponseDiff,
  },
};

export const TextBodyChanged: Story = {
  args: {
    diff: {
      status: null,
      header_changes: [],
      body: {
        kind: "Text",
        lines: [
          { kind: "Unchanged", line: "OK" },
          { kind: "Removed", line: "user: chris" },
          { kind: "Added", line: "user: chris.conlon" },
        ],
      },
      identical: false,
    } satisfies ResponseDiff,
  },
};
