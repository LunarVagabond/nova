import type { Meta, StoryObj } from "@storybook/vue3-vite";
import { ref } from "vue";
import MultipartEditor from "./MultipartEditor.vue";
import type { MultipartField } from "../types/nova";

const meta: Meta<typeof MultipartEditor> = {
  title: "Components/MultipartEditor",
  component: MultipartEditor,
  argTypes: {
    "onUpdate:modelValue": { action: "update:modelValue" },
  },
};

export default meta;
type Story = StoryObj<typeof MultipartEditor>;

function withLocalState(initial: MultipartField[], projectRoot: string) {
  return {
    render: (args: { modelValue: MultipartField[]; projectRoot: string }) => ({
      components: { MultipartEditor },
      setup() {
        const value = ref(args.modelValue);
        return { args, value };
      },
      template: `<MultipartEditor :model-value="value" :project-root="args.projectRoot" @update:model-value="value = $event" />`,
    }),
    args: { modelValue: initial, projectRoot },
  };
}

export const Empty: Story = withLocalState([], "/home/chris/projects/nova-fixtures");

export const MixedTextAndFileFields: Story = withLocalState(
  [
    { name: "username", filename: null, content_type: null, value: "chris.conlon", file_path: null },
    { name: "avatar", filename: "avatar.png", content_type: "image/png", value: "", file_path: "fixtures/avatar.png" },
    { name: "bio", filename: null, content_type: null, value: "", file_path: null },
  ],
  "/home/chris/projects/nova-fixtures",
);
