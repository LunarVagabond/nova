<script setup lang="ts">
// Browses an introspected GraphQL schema (Query/Mutation/Subscription field
// trees) next to the query editor, and lets a field be inserted into the
// query at the cursor rather than requiring it to be typed from memory or
// looked up elsewhere. Introspection itself, caching, and every type this
// works with are owned by nova-engine (`fetchGraphqlSchema`) — this
// component only renders what comes back and reports what was clicked.
import { computed, ref } from "vue";

import type { GraphQlFieldDef, GraphQlSchema, GraphQlTypeDef } from "../types/nova";
import Icon from "./Icon.vue";

const props = defineProps<{
  schema: GraphQlSchema | null;
  loading: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (e: "refresh"): void;
  (e: "insert", text: string): void;
}>();

interface RootSection {
  label: string;
  typeDef: GraphQlTypeDef | undefined;
}

const rootSections = computed<RootSection[]>(() => {
  const schema = props.schema;
  if (!schema) return [];
  const typeByName = new Map(schema.types.map((t) => [t.name, t]));
  return [
    { label: "Query", name: schema.query_type },
    { label: "Mutation", name: schema.mutation_type },
    { label: "Subscription", name: schema.subscription_type },
  ]
    .filter((root): root is { label: string; name: string } => root.name !== null)
    .map((root) => ({ label: root.label, typeDef: typeByName.get(root.name) }));
});

const expandedSections = ref<Set<string>>(new Set(["Query"]));

function toggleSection(label: string) {
  const next = new Set(expandedSections.value);
  if (next.has(label)) {
    next.delete(label);
  } else {
    next.add(label);
  }
  expandedSections.value = next;
}

function fieldTooltip(field: GraphQlFieldDef): string {
  const parts = [`${field.name}: ${field.type_ref}`];
  if (field.description) parts.push(field.description);
  if (field.args.length > 0) {
    parts.push(
      "Args: " + field.args.map((a) => `${a.name}: ${a.type_ref}`).join(", "),
    );
  }
  return parts.join("\n");
}

// Base type name a field's `type_ref` resolves to, stripped of `!`/`[]`
// wrappers — used only to decide whether the inserted snippet should offer
// a selection-set body (`{ }`) for an object-typed field.
function baseTypeName(typeRef: string): string {
  return typeRef.replace(/[[\]!]/g, "");
}

function isObjectType(typeRef: string): boolean {
  const schema = props.schema;
  if (!schema) return false;
  const base = baseTypeName(typeRef);
  return schema.types.some((t) => t.name === base && t.kind === "OBJECT");
}

function insertField(field: GraphQlFieldDef) {
  let text = field.name;
  if (field.args.length > 0) {
    text += `(${field.args.map((a) => `${a.name}: `).join(", ")})`;
  }
  if (isObjectType(field.type_ref)) {
    text += " {\n  \n}";
  }
  emit("insert", text);
}
</script>

<template>
  <div class="graphql-schema-explorer">
    <div class="graphql-schema-explorer__header">
      <span class="graphql-schema-explorer__title">Schema</span>
      <button
        type="button"
        class="icon-button icon-button--outline"
        :title="schema ? 'Refresh schema' : 'Fetch schema'"
        :disabled="loading"
        @click="emit('refresh')"
      >
        <Icon name="history" />
      </button>
    </div>

    <p v-if="loading" class="graphql-schema-explorer__hint">Fetching schema…</p>
    <p v-else-if="error" class="graphql-schema-explorer__error">{{ error }}</p>
    <p v-else-if="!schema" class="graphql-schema-explorer__hint">
      Fetch the schema to browse its queries and mutations.
    </p>
    <p v-else-if="rootSections.length === 0" class="graphql-schema-explorer__hint">
      This schema has no Query, Mutation, or Subscription type.
    </p>

    <div v-else class="graphql-schema-explorer__tree">
      <div v-for="section in rootSections" :key="section.label" class="graphql-schema-explorer__section">
        <button
          type="button"
          class="graphql-schema-explorer__section-toggle"
          @click="toggleSection(section.label)"
        >
          <Icon
            name="chevron-down"
            class="graphql-schema-explorer__chevron"
            :class="{ 'graphql-schema-explorer__chevron--collapsed': !expandedSections.has(section.label) }"
          />
          {{ section.label }}
        </button>
        <ul v-if="expandedSections.has(section.label)" class="graphql-schema-explorer__fields">
          <li v-for="field in section.typeDef?.fields ?? []" :key="field.name">
            <button
              type="button"
              class="graphql-schema-explorer__field"
              :title="fieldTooltip(field)"
              @click="insertField(field)"
            >
              <span class="graphql-schema-explorer__field-name">{{ field.name }}</span>
              <span class="graphql-schema-explorer__field-type">{{ field.type_ref }}</span>
            </button>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>
