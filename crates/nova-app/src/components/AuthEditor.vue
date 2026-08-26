<script setup lang="ts">
import { computed } from "vue";

import type { ApiKeyLocation, AuthScheme, AuthSchemeType } from "../types/nova";

/**
 * Editor for a structured auth scheme, shared by the request panel's Auth
 * tab and the environment panel's default-auth section. Follows the same
 * shape as the Body tab's content-type selector: pick a type from a
 * dropdown, and the fields that type needs render underneath it.
 *
 * `null` means "no auth declared" — for a request, no `[auth]` section at
 * all; for an environment, no `auth:` key.
 */
const props = defineProps<{
  modelValue: AuthScheme | null;
  /** Prefix for input ids, so two editors on one page don't collide. */
  idPrefix: string;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: AuthScheme | null): void;
}>();

type Selection = "none" | AuthSchemeType;

const AUTH_TYPE_OPTIONS: Selection[] = [
  "none",
  "bearer",
  "basic",
  "api_key",
  "oauth2_client_credentials",
];

const AUTH_TYPE_LABELS: Record<Selection, string> = {
  none: "No Auth",
  bearer: "Bearer Token",
  basic: "Basic Auth",
  api_key: "API Key",
  oauth2_client_credentials: "OAuth2 Client Credentials",
};

const API_KEY_LOCATIONS: ApiKeyLocation[] = ["header", "query"];

const API_KEY_LOCATION_LABELS: Record<ApiKeyLocation, string> = {
  header: "Header",
  query: "Query Param",
};

const selectedType = computed<Selection>(() => props.modelValue?.type ?? "none");

/** A blank scheme of `type`, with every field empty and ready to fill in. */
function blankScheme(type: Selection): AuthScheme | null {
  switch (type) {
    case "none":
      return null;
    case "bearer":
      return { type: "bearer", token: "" };
    case "basic":
      return { type: "basic", username: "", password: "" };
    case "api_key":
      return { type: "api_key", name: "", value: "", location: "header" };
    case "oauth2_client_credentials":
      return {
        type: "oauth2_client_credentials",
        token_url: "",
        client_id: "",
        client_secret: "",
        scope: null,
      };
  }
}

// Switching type replaces the scheme wholesale rather than carrying fields
// across — the variants share no meaningful fields, so a half-migrated
// scheme would only be confusing.
function handleTypeChange(next: Selection) {
  if (next === selectedType.value) return;
  emit("update:modelValue", blankScheme(next));
}

/**
 * A writable view of one string field of whichever scheme is selected.
 * Reads back "" when the current scheme has no such field, and emits a
 * whole new scheme object on write so the parent's dirty-tracking (a plain
 * comparison against the last saved snapshot) notices the change.
 */
function schemeField(key: string) {
  return computed<string>({
    get() {
      const value = (props.modelValue as Record<string, unknown> | null)?.[key];
      return typeof value === "string" ? value : "";
    },
    set(value: string) {
      if (!props.modelValue) return;
      emit("update:modelValue", { ...props.modelValue, [key]: value } as AuthScheme);
    },
  });
}

const token = schemeField("token");
const username = schemeField("username");
const password = schemeField("password");
const apiKeyName = schemeField("name");
const apiKeyValue = schemeField("value");
const tokenUrl = schemeField("token_url");
const clientId = schemeField("client_id");
const clientSecret = schemeField("client_secret");

// `scope` is genuinely optional, so a blank field means "don't send one"
// rather than "send an empty scope".
const scope = computed<string>({
  get() {
    return props.modelValue?.type === "oauth2_client_credentials"
      ? (props.modelValue.scope ?? "")
      : "";
  },
  set(value: string) {
    if (props.modelValue?.type !== "oauth2_client_credentials") return;
    emit("update:modelValue", { ...props.modelValue, scope: value.trim() === "" ? null : value });
  },
});

const apiKeyLocation = computed<ApiKeyLocation>({
  get() {
    return props.modelValue?.type === "api_key" ? props.modelValue.location : "header";
  },
  set(value: ApiKeyLocation) {
    if (props.modelValue?.type !== "api_key") return;
    emit("update:modelValue", { ...props.modelValue, location: value });
  },
});
</script>

<template>
  <div>
    <div class="request-panel__body-type">
      <span class="request-panel__body-type-label">Type</span>
      <select
        class="request-panel__body-type-select"
        :value="selectedType"
        @change="handleTypeChange(($event.target as HTMLSelectElement).value as Selection)"
      >
        <option v-for="option in AUTH_TYPE_OPTIONS" :key="option" :value="option">
          {{ AUTH_TYPE_LABELS[option] }}
        </option>
      </select>
    </div>

    <p v-if="selectedType === 'none'" class="request-panel__hint-text">
      No authentication is applied here.
    </p>

    <div v-else class="manifest-editor">
      <template v-if="modelValue?.type === 'bearer'">
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-token`">Token</label>
          <input
            :id="`${idPrefix}-token`"
            v-model="token"
            type="text"
            class="manifest-editor__input"
            placeholder="{{access_token}}"
          />
        </div>
      </template>

      <template v-else-if="modelValue?.type === 'basic'">
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-username`">Username</label>
          <input
            :id="`${idPrefix}-username`"
            v-model="username"
            type="text"
            class="manifest-editor__input"
            placeholder="{{username}}"
          />
        </div>
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-password`">Password</label>
          <input
            :id="`${idPrefix}-password`"
            v-model="password"
            type="text"
            class="manifest-editor__input"
            placeholder="{{password}}"
          />
        </div>
      </template>

      <template v-else-if="modelValue?.type === 'api_key'">
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-key-name`">Key</label>
          <input
            :id="`${idPrefix}-key-name`"
            v-model="apiKeyName"
            type="text"
            class="manifest-editor__input"
            placeholder="X-API-Key"
          />
        </div>
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-key-value`">Value</label>
          <input
            :id="`${idPrefix}-key-value`"
            v-model="apiKeyValue"
            type="text"
            class="manifest-editor__input"
            placeholder="{{api_key}}"
          />
        </div>
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-key-location`">Add to</label>
          <select
            :id="`${idPrefix}-key-location`"
            v-model="apiKeyLocation"
            class="request-panel__body-type-select"
          >
            <option v-for="option in API_KEY_LOCATIONS" :key="option" :value="option">
              {{ API_KEY_LOCATION_LABELS[option] }}
            </option>
          </select>
        </div>
      </template>

      <template v-else-if="modelValue?.type === 'oauth2_client_credentials'">
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-token-url`">Token URL</label>
          <input
            :id="`${idPrefix}-token-url`"
            v-model="tokenUrl"
            type="text"
            class="manifest-editor__input"
            placeholder="{{token_url}}"
          />
        </div>
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-client-id`">Client ID</label>
          <input
            :id="`${idPrefix}-client-id`"
            v-model="clientId"
            type="text"
            class="manifest-editor__input"
            placeholder="{{client_id}}"
          />
        </div>
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-client-secret`">
            Client Secret
          </label>
          <input
            :id="`${idPrefix}-client-secret`"
            v-model="clientSecret"
            type="text"
            class="manifest-editor__input"
            placeholder="{{client_secret}}"
          />
        </div>
        <div class="manifest-editor__field">
          <label class="manifest-editor__label" :for="`${idPrefix}-scope`">
            Scope <span class="auth-editor__optional">optional</span>
          </label>
          <input
            :id="`${idPrefix}-scope`"
            v-model="scope"
            type="text"
            class="manifest-editor__input"
            placeholder="read write"
          />
        </div>
        <p class="request-panel__hint-text">
          The client ID and secret are exchanged for an access token when the request is sent.
          The token is reused for the rest of the run until it expires.
        </p>
      </template>
    </div>
  </div>
</template>
