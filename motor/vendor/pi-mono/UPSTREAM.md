# Origem do código vendorizado

- Repositório: https://github.com/badlogic/pi-mono
- Commit: `22fc026b872b4a7df22ffa94e8fa00b873128054` (15/11/2026)
- Licença: MIT (Mario Zechner) — `LICENSE` nesta pasta
- Copiado em: 16/11/2026 (fatia 1 do Motor Mexerica, MEX-1293/MEX-1359)

## O que foi copiado

| Pacote | Conteúdo copiado |
|---|---|
| `packages/schema` | `src/` inteiro |
| `packages/telemetry` | `src/` inteiro |
| `packages/agent` | `src/` inteiro |
| `packages/ai` | subconjunto de `src/` (57 + 5 arquivos): tipos, `models`, `models-store`, `session-resources`, `auth/*`, `api/*` sem catálogos gerados, `providers/{faux,model-schema,compat-schema,schema-builders}`, `utils/*` |
| `packages/coding-agent` | somente `src/modes/rpc/jsonl.ts` (enquadramento JSONL do modo RPC) |

Não entram nesta fatia: `tui`, `mcp`, `codemode`, `env`, `durable`, `chord`,
`evals`, os `*.models.ts` e dados de catálogo (`src/providers/data/`,
`models.generated.ts`, `providers/all.ts`), `compat.ts` e os fluxos OAuth de
`auth/oauth/` — ficam para as fatias de login/transporte.

## Como atualizar

Um PR por atualização, nunca automático: comparar este commit com o novo só nas
pastas copiadas, aplicar o diff, reaplicar `PATCHES.md`, rodar `npm run
typecheck` e `npm test` em `motor/` e registrar o commit novo aqui e em
`THIRD_PARTY.md`. Correção de segurança do upstream: PR no mesmo dia.
