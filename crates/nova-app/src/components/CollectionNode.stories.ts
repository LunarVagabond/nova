import type { Meta, StoryObj } from "@storybook/vue3-vite";
import CollectionNode from "./CollectionNode.vue";
import type { Collection, GitStatusMap } from "../types/nova";

const SAMPLE_TREE: Collection = {
  name: "collections",
  path: "/home/chris/projects/nova-fixtures/nova/collections",
  children: [
    {
      name: "auth",
      path: "/home/chris/projects/nova-fixtures/nova/collections/auth",
      children: [],
      requests: [
        {
          name: "login",
          path: "/home/chris/projects/nova-fixtures/nova/collections/auth/login.nova",
          method: "POST",
          protocol: "http",
        },
      ],
    },
    {
      name: "users",
      path: "/home/chris/projects/nova-fixtures/nova/collections/users",
      children: [],
      requests: [
        {
          name: "create",
          path: "/home/chris/projects/nova-fixtures/nova/collections/users/create.nova",
          method: "POST",
          protocol: "http",
        },
        {
          name: "get",
          path: "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova",
          method: "GET",
          protocol: "http",
        },
        {
          name: "live-updates",
          path: "/home/chris/projects/nova-fixtures/nova/collections/users/live-updates.nova",
          method: "",
          protocol: "websocket",
        },
      ],
    },
  ],
  requests: [],
};

const GIT_STATUS: GitStatusMap = {
  "/home/chris/projects/nova-fixtures/nova/collections/users/create.nova": "unstaged",
  "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova": "staged",
};

const meta: Meta<typeof CollectionNode> = {
  title: "Components/CollectionNode",
  component: CollectionNode,
  argTypes: {
    onSelect: { action: "select" },
    onCreateRequest: { action: "createRequest" },
    onCreateCollection: { action: "createCollection" },
    onRenameCollection: { action: "renameCollection" },
    onDeleteCollection: { action: "deleteCollection" },
    onRenameRequest: { action: "renameRequest" },
    onDuplicateRequest: { action: "duplicateRequest" },
    onDeleteRequest: { action: "deleteRequest" },
  },
};

export default meta;
type Story = StoryObj<typeof CollectionNode>;

export const RootTree: Story = {
  args: {
    collection: SAMPLE_TREE,
    isRoot: true,
  },
};

export const WithGitStatusAndSelection: Story = {
  args: {
    collection: SAMPLE_TREE,
    isRoot: true,
    gitStatus: GIT_STATUS,
    selectedPath: "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova",
  },
};

export const Filtered: Story = {
  args: {
    collection: SAMPLE_TREE,
    isRoot: true,
    filter: "login",
  },
};

export const EmptyCollection: Story = {
  args: {
    collection: { name: "collections", path: "/tmp/empty", children: [], requests: [] },
    isRoot: true,
  },
};
