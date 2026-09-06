import type { Meta, StoryObj } from "@storybook/vue3-vite";
import { ref } from "vue";
import AuthEditor from "./AuthEditor.vue";
import type { AuthScheme } from "../types/nova";

const meta: Meta<typeof AuthEditor> = {
  title: "Components/AuthEditor",
  component: AuthEditor,
  args: {
    idPrefix: "story-auth",
    projectRoot: "/home/chris/projects/nova-fixtures",
  },
  argTypes: {
    "onUpdate:modelValue": { action: "update:modelValue" },
  },
};

export default meta;
type Story = StoryObj<typeof AuthEditor>;

function withLocalState(initial: AuthScheme | null) {
  return {
    render: (args: { idPrefix: string; projectRoot: string }) => ({
      components: { AuthEditor },
      setup() {
        const value = ref<AuthScheme | null>(initial);
        return { args, value };
      },
      template: `<AuthEditor v-bind="args" :model-value="value" @update:model-value="value = $event" />`,
    }),
    args: {},
  };
}

export const NoAuth: Story = withLocalState(null);

export const BearerToken: Story = withLocalState({ type: "bearer", token: "{{access_token}}" });

export const BasicAuth: Story = withLocalState({ type: "basic", username: "{{username}}", password: "{{password}}" });

export const ApiKey: Story = withLocalState({ type: "api_key", name: "X-API-Key", value: "{{api_key}}", location: "header" });

export const OAuth2ClientCredentials: Story = withLocalState({
  type: "oauth2_client_credentials",
  token_url: "{{token_url}}",
  client_id: "{{client_id}}",
  client_secret: "{{client_secret}}",
  scope: "read write",
});

// This form's "Get New Access Token" button calls `oauth2_authorization_status`
// on mount (see AuthEditor.vue's `refreshAuthorizationStatus`) — no Tauri
// mock is registered here, so it fails harmlessly and the form always shows
// "not authorized" rather than a real status.
export const OAuth2AuthorizationCode: Story = withLocalState({
  type: "oauth2_authorization_code",
  auth_url: "{{auth_url}}",
  token_url: "{{token_url}}",
  client_id: "{{client_id}}",
  client_secret: "{{client_secret}}",
  scope: null,
});

export const DigestAuth: Story = withLocalState({ type: "digest", username: "{{username}}", password: "{{password}}" });
