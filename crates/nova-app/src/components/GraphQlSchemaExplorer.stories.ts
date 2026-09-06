import type { Meta, StoryObj } from "@storybook/vue3-vite";
import GraphQlSchemaExplorer from "./GraphQlSchemaExplorer.vue";
import type { GraphQlSchema } from "../types/nova";

const SAMPLE_SCHEMA: GraphQlSchema = {
  query_type: "Query",
  mutation_type: "Mutation",
  subscription_type: null,
  types: [
    {
      name: "Query",
      kind: "OBJECT",
      description: null,
      fields: [
        { name: "user", description: "Look up a single user by id.", args: [{ name: "id", description: null, type_ref: "ID!" }], type_ref: "User" },
        { name: "users", description: null, args: [], type_ref: "[User!]!" },
      ],
    },
    {
      name: "Mutation",
      kind: "OBJECT",
      description: null,
      fields: [
        { name: "createUser", description: null, args: [{ name: "input", description: null, type_ref: "CreateUserInput!" }], type_ref: "User" },
      ],
    },
    {
      name: "User",
      kind: "OBJECT",
      description: null,
      fields: [
        { name: "id", description: null, args: [], type_ref: "ID!" },
        { name: "name", description: null, args: [], type_ref: "String!" },
      ],
    },
  ],
};

const meta: Meta<typeof GraphQlSchemaExplorer> = {
  title: "Components/GraphQlSchemaExplorer",
  component: GraphQlSchemaExplorer,
  args: {
    loading: false,
    error: null,
  },
  argTypes: {
    onRefresh: { action: "refresh" },
    onInsert: { action: "insert" },
  },
};

export default meta;
type Story = StoryObj<typeof GraphQlSchemaExplorer>;

export const NotFetchedYet: Story = {
  args: { schema: null },
};

export const Loading: Story = {
  args: { schema: null, loading: true },
};

export const Loaded: Story = {
  args: { schema: SAMPLE_SCHEMA },
};

export const FetchFailed: Story = {
  args: { schema: null, error: "Introspection query failed: 404 Not Found" },
};
