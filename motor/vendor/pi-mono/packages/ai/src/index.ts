// Subconjunto da fatia 1 do Motor Mexerica (ver PATCHES.md): reexporta só o
// que `pi-agent-core` e `motor/mexerica` usam — sem catálogos gerados,
// `compat` nem o registro `providers/all`.
export * from "./auth/context.ts";
export * from "./auth/credential-store.ts";
export * from "./auth/helpers.ts";
export * from "./auth/types.ts";
export * from "./models.ts";
export * from "./models-store.ts";
export * from "./providers/faux.ts";
export * from "./providers/model-schema.ts";
export * from "./session-resources.ts";
export * from "./types.ts";
export * from "./utils/diagnostics.ts";
export * from "./utils/event-stream.ts";
export * from "./utils/identity.ts";
export * from "./utils/json-parse.ts";
export * from "./utils/model-operations.ts";
export * from "./utils/streaming-json.ts";
export * from "./utils/text.ts";
export * from "./utils/transcript.ts";
export { uuidv7 } from "./utils/uuid.ts";
export { PI_AI_VERSION } from "./utils/version.ts";
export * from "./utils/overflow.ts";
export * from "./utils/string-enum.ts";
export * from "./utils/validation.ts";
