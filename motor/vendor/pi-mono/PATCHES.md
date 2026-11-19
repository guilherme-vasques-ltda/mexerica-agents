# Mudanças nossas dentro da cópia (PATCHES)

Regra do fork: mudança no vendor só como gancho curto. Cada item lista arquivo,
o que mudou e por quê.

1. `packages/*/package.json` — `exports` repontados de `dist/` para `src/*.ts`
   (rodando direto por `node --experimental-strip-types`, sem etapa de build).
   Em `schema`: `./formats` → `src/formats/index.ts`. Em `telemetry`:
   `./render` e `./testing` preservados.
2. `packages/ai/package.json` — dependências reduzidas ao que a fatia usa:
   `@anthropic-ai/sdk`, `@google/genai`, `openai`, `partial-json`,
   `http-proxy-agent`, `https-proxy-agent` e os pacotes internos. Saíram
   `@aws-sdk/*` e `@smithy/*` (Bedrock fora da fatia).
3. `packages/coding-agent/package.json` — reduzido a nome/versão/tipo: só
   `src/modes/rpc/jsonl.ts` foi copiado.
4. `packages/ai/src/index.ts` — NOVO (substitui o de upstream, que não foi
   copiado): reexporta só o subconjunto da fatia, sem `compat`, `providers/all`
   nem catálogos gerados.
5. `packages/ai/src/api/bedrock-converse-stream.ts` — SUBSTITUÍDO por stub só de
   tipos (`BedrockOptions`, `BedrockThinkingDisplay`), porque `types.ts`
   referencia esses tipos e o SDK AWS não entra nesta fatia.
6. `packages/{agent,ai,schema,telemetry}/package.json` — `devDependencies`
   upstream removidas por inteiro (canvas, vitest, typebox, validadores de
   benchmark): as suítes do upstream não foram copiadas e os testes da Mexerica
   usam `node:test` com o `typescript` da raiz do motor. Menos pacotes na
   instalação, menos scripts e menos superfície de auditoria.

Nota de instalação (não é patch, é registro): `npm ci` em `motor/` ainda executa
os scripts de instalação de duas dependências transitivas — `@google/genai`
(`preinstall`, um `echo` no-op) e `protobufjs` (`postinstall`, checagem local de
arquivos do próprio pacote). Não afirmamos instalação sem scripts.
