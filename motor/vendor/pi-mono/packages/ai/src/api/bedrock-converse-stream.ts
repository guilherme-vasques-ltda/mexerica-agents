// Stub Mexerica (PATCHES.md): Bedrock não entra na fatia 1 e o SDK AWS não é
// dependência. Mantidos só os tipos que `types.ts` referencia; a implementação
// upstream volta junto se uma fatia futura ativar o provedor.
import type { ModelThinkingLevel as ThinkingLevel } from "../providers/model-schema.ts";
import type { StreamOptions, ThinkingBudgets } from "../types.ts";

export type BedrockThinkingDisplay = "summarized" | "omitted";

export interface BedrockOptions extends StreamOptions {
	region?: string;
	profile?: string;
	toolChoice?: "auto" | "any" | "none" | { type: "tool"; name: string };
	reasoning?: ThinkingLevel;
	thinkingBudgets?: ThinkingBudgets;
	interleavedThinking?: boolean;
	thinkingDisplay?: BedrockThinkingDisplay;
	requestMetadata?: Record<string, string>;
	bearerToken?: string;
}
