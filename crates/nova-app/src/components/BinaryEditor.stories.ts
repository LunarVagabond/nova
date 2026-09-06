import type { Meta, StoryObj } from "@storybook/vue3-vite";
import { ref } from "vue";
import BinaryEditor from "./BinaryEditor.vue";

const meta: Meta<typeof BinaryEditor> = {
  title: "Components/BinaryEditor",
  component: BinaryEditor,
  argTypes: {
    "onUpdate:modelValue": { action: "update:modelValue" },
  },
};

export default meta;
type Story = StoryObj<typeof BinaryEditor>;

// `chooseFile` calls the Tauri file-picker dialog, which isn't available
// here — clicking "Choose file"/"Change file" will reject harmlessly.
export const NoFileChosen: Story = {
  args: {
    modelValue: null,
    projectRoot: "/home/chris/projects/nova-fixtures",
  },
};

export const FileChosen: Story = {
  render: (args) => ({
    components: { BinaryEditor },
    setup() {
      const value = ref(args.modelValue);
      return { args, value };
    },
    template: `<BinaryEditor :model-value="value" :project-root="args.projectRoot" @update:model-value="value = $event" />`,
  }),
  args: {
    modelValue: "fixtures/sample-payload.bin",
    projectRoot: "/home/chris/projects/nova-fixtures",
  },
};
