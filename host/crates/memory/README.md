# Mexerica — biblioteca de memória

Documento adaptado em 28/10/2026. O pacote Rust conserva o nome técnico `codync-mem` e o módulo `codync_mem`. É uma biblioteca usada pelo Hub; este fork não anuncia uma publicação própria em registro de pacotes ou o espelho independente do upstream.

## Responsabilidade

A biblioteca mantém entradas de memória da pessoa e de cada bot, notas longas, histórico de alterações, verificações de escrita e a seleção de contexto de abertura. A conversa completa permanece no transcript; conhecimento do projeto tem seus arquivos próprios. As regras deste repositório estão em AGENTS.md, e não autorizam criar memórias pessoais do harness.

O chamador fornece a conexão SQLite e a transação. A biblioteca não inicia processos, rede, modelos ou threads; tarefas de modelo e efeitos externos pertencem ao chamador. Regras de ferramenta/permissão do Hub continuam sendo necessárias: o crate não concede autoridade a um bot.

## Integração técnica

```rust
use codync_mem::rusqlite::Connection;

let mut conn = Connection::open_in_memory()?;
codync_mem::schema::migrate(&conn)?;
```

Use o `rusqlite` reexportado pela biblioteca para evitar vincular duas versões incompatíveis de SQLite. A busca usa FTS5/trigram e `remove_diacritics`, conforme os requisitos do SQLite incorporado. A dependência e o nome do crate continuam definidos no `Cargo.toml`; não os altere por uma troca de título.

Todo Cargo do Host, inclusive desta biblioteca, roda apenas no servidor pelo fluxo de AGENTS.md. Mudanças no fork vão por PR para `main` do repositório privado, não para uma branch `dev` do upstream. Não há instalação ou publicação de crate autorizada por este README.

## Módulos

| Módulo | Função |
|---|---|
| `store` | Entradas por camada, arquivo, esquecimento com tombstones e busca |
| `check` | Verificações determinísticas de escrita |
| `opening` | Contexto de memória de abertura dentro dos limites |
| `files` | Notas longas, versões, autoria, causa, diffs e índice |
| `dream` | Preparação, auditoria e aplicação registrada das sugestões entre turnos |
| `evidence` | Identificação de palavras da própria pessoa, separadas de conteúdo colado/citado |
| `redact` | Redação de padrões de credenciais |
| `text` | Comparação sem diferença de caixa ou acentos |
| `compaction` | Preparação de resumo, guardas e passagem a outra conversa |
| `tokens` | Estimativa usada pelos limites de contexto |

## Alterações do fork e origem

O fork acrescenta padrões em português do Brasil em `check`/`evidence`, redação de segredos e comparação textual, além da busca sem acentos. Padrões de `ai-memory` adaptados sob MIT estão registrados em [THIRD_PARTY.md](../../../THIRD_PARTY.md). Migrações do fork usam a chave `donnabot` em `memory_meta`, separada da evolução da base.

A origem Codync, Po Kai Lee e contribuidores permanece em [NOTICE](../../../NOTICE). A licença Apache-2.0 está em [LICENSE](LICENSE) desta biblioteca e em [LICENSE da raiz](../../../LICENSE); avisos específicos de material adaptado continuam aplicáveis. Nenhuma licença ou copyright foi substituído nesta edição.
