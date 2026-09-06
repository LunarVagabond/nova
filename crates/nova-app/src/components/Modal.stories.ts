import type { Meta, StoryObj } from "@storybook/vue3-vite";
import Modal from "./Modal.vue";

const meta: Meta<typeof Modal> = {
  title: "Components/Modal",
  component: Modal,
  argTypes: {
    onCancel: { action: "cancel" },
  },
};

export default meta;
type Story = StoryObj<typeof Modal>;

export const Default: Story = {
  args: {
    title: "Delete request?",
  },
  render: (args) => ({
    components: { Modal },
    setup() {
      return { args };
    },
    template: `
      <Modal v-bind="args" @cancel="args.cancel">
        This can't be undone.
        <template #actions>
          <button class="button button--ghost" @click="args.cancel">Cancel</button>
          <button class="button button--danger">Delete</button>
        </template>
      </Modal>
    `,
  }),
};

export const Wide: Story = {
  args: {
    title: "Test results",
    wide: true,
  },
  render: (args) => ({
    components: { Modal },
    setup() {
      return { args };
    },
    template: `
      <Modal v-bind="args" @cancel="args.cancel">
        <p>3 passed, 1 failed.</p>
        <template #actions>
          <button class="button" @click="args.cancel">Close</button>
        </template>
      </Modal>
    `,
  }),
};
