import type { Meta, StoryObj } from "@storybook/vue3-vite";
import ResponseTimelineView from "./ResponseTimelineView.vue";

const meta: Meta<typeof ResponseTimelineView> = {
  title: "Components/ResponseTimelineView",
  component: ResponseTimelineView,
};

export default meta;
type Story = StoryObj<typeof ResponseTimelineView>;

export const FastResponse: Story = {
  args: {
    timing: { time_to_first_byte_ms: 42, content_download_ms: 8 },
  },
};

export const SlowServer: Story = {
  args: {
    timing: { time_to_first_byte_ms: 1840, content_download_ms: 120 },
  },
};

export const LargeDownload: Story = {
  args: {
    timing: { time_to_first_byte_ms: 65, content_download_ms: 950 },
  },
};
