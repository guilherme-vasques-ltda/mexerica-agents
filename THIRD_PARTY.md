# Mexerica — procedência e atribuições

O histórico, `LICENSE` e `NOTICE` preservam a origem do fork Codync (Apache-2.0).
A identificação Mexerica (antes DONNA Bot, D443) não substitui licenças de componentes, titulares,
avisos por arquivo ou condições de marca/serviços. A licença Apache-2.0 da base
não se estende por inferência a todo código, fonte, skill ou ativo do conjunto.

Revisão documental de 28/10/2026: READMEs e regras próprios adaptados para a marca;
texto integral de LICENSE e avisos originais de NOTICE preservados. Nenhuma
dependência, lockfile, ativo ou corpo de skill foi incorporado por esta edição.
O crédito herdado de inspiração da ilha de desktop em @novra.design permanece
no README principal. A exclusão do CODE_OF_CONDUCT retira o documento Contributor
Covenant 3.0 (CC BY-SA 4.0); nenhum trecho dele foi incorporado aos textos novos.
O relato de segurança e a confidencialidade permanecem em SECURITY.md/AGENTS.md.

Registre abaixo cada nova cópia de código de referência, inclusive adaptações,
conforme D75.

| Origem (repositório/URL) | Commit | Arquivo de origem | Arquivo de destino | Licença | Data (AAAA-MM-DD) |
|---|---|---|---|---|---|
| [Exa MCP Server](https://github.com/exa-labs/exa-mcp-server) — Exa Labs | `a02cd621534313b8f01dbb03b829dac448decb8b` | `skills/search/SKILL.md`, `LICENSE` | `host/catalogo/skills/exa/exa-pesquisar-na-web/` (adaptação curta e aviso MIT) | [MIT](https://github.com/exa-labs/exa-mcp-server/blob/a02cd621534313b8f01dbb03b829dac448decb8b/LICENSE) | 2026-11-18 |
| [Supabase Agent Skills](https://github.com/supabase/agent-skills) — Supabase | `c9be0e931b7930f7d02126d04774d904c381e7d7` | `skills/supabase/SKILL.md`, `LICENSE` | `host/catalogo/skills/supabase/supabase-revisar-seguranca/` (adaptação curta e aviso MIT) | [MIT](https://github.com/supabase/agent-skills/blob/c9be0e931b7930f7d02126d04774d904c381e7d7/LICENSE) | 2026-11-18 |
| [Codync](https://github.com/leepokai/Codync) — Kika | `b365471af789fb9b9e3822b16049d5e4c1a7b5a4` | `host/src/usage/claude.rs` (isolamento MCP de `/usage`) | `host/src/usage.rs` (arquivo modificado: flags adaptadas à chamada Donna existente, sem reorganizar usage) | [Apache-2.0](https://github.com/leepokai/Codync/blob/b365471af789fb9b9e3822b16049d5e4c1a7b5a4/LICENSE) | 2026-11-03 |
| [Codync](https://github.com/leepokai/Codync) — leepokai <109857817+leepokai@users.noreply.github.com>; Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com> (ambos os commits; metadados conferidos via gh api) | `653e42d2850dcc85da52cbf37ebd315f21cb65f9`, `bdc60bd96b500c0a1b0a178124297a23688163a9` | `host/src/market/oauth.rs` (comparação de alvo em callback/refresh/saída e fronteiras de teste) | `host/src/market/oauth.rs` (modificado; porte seletivo adaptado a CAS de instância/concessão Donna; testes próprios em `host/src/fork/conectores/oauth_tests.rs`; binding/autoridade, testemunho pré-unlock, Context/Setup do runtime, encaixe seguro do comparador/upgrade bootstrap (2026-11-04) e adaptação da política legada do cofre são autorais Donna) | [Apache-2.0](https://github.com/leepokai/Codync/blob/bdc60bd96b500c0a1b0a178124297a23688163a9/LICENSE) | 2026-11-03 |
| [Codync](https://github.com/leepokai/Codync) — leepokai <109857817+leepokai@users.noreply.github.com>; Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com> | `e82c417424e06d1da71a865845f230d665b59ff1` | `host/src/market/oauth.rs` (separação POST/classificação, renovação conservando login e resource); `host/src/market/oauth/tests.rs` (fronteiras de renovação) | `host/src/market/oauth.rs`, `host/src/fork/conectores/oauth_refresh.rs`, `host/src/fork/conectores/oauth_tests.rs` (arquivos modificados/derivados; classificação estrita, PT-BR, fixture HTTP real, resource exato e integração nas guardas/CAS do02 adaptados pela Mexerica; logs externos inseguros e regra genérica de revogação não portados; docs marketplace/web não copiadas) | [Apache-2.0](https://github.com/leepokai/Codync/blob/e82c417424e06d1da71a865845f230d665b59ff1/LICENSE) | 2026-11-04 |
| [Codync](https://github.com/leepokai/Codync) — leepokai <109857817+leepokai@users.noreply.github.com>; coautoria histórica Claude Opus 5.5 <noreply@anthropic.com> | `6dcee39da4e475d233bc9c245a9505bd0c10f27b` | `apps/desktop/src/main/files.ts`, `apps/desktop/src/renderer/views/thread/Composer.tsx`, `composer-files.ts`, `composer-files.test.ts` (os dois últimos em `apps/desktop/src/renderer/views/thread/`) | `apps/desktop/src/main/files.ts`, `apps/desktop/src/renderer/views/thread/Composer.tsx`, `fork-composer-files.ts`, `fork-composer-files.test.ts` (os dois últimos na mesma pasta); adaptação seletiva às guardas e à origem Mexerica | [Apache-2.0](https://github.com/leepokai/Codync/blob/6dcee39da4e475d233bc9c245a9505bd0c10f27b/LICENSE) | 2026-11-04 |
| [Codync](https://github.com/leepokai/Codync) — leepokai <109857817+leepokai@users.noreply.github.com>; Co-Authored-By histórico: Claude Opus 5.5 <noreply@anthropic.com> (2026-11-02T16:27:50Z) | `219935410dffccb950f90d0565bd1d9f1661bd5b` | `apps/desktop/src/renderer/components/Overlay.tsx`, `lib/modal-stack.ts`, `lib/modal-stack.test.ts`, `views/bots/parts.tsx` (os três últimos relativos a `apps/desktop/src/renderer/`); trecho de interação de `(arquivo de design privado do projeto de origem, fora deste repositório)` | `apps/desktop/src/renderer/components/Overlay.tsx`, `lib/fork-modal-stack.ts`, `lib/fork-modal-stack.test.ts`, `views/bots/parts.tsx` (mesma raiz renderer); `(arquivo de design privado do projeto de origem, fora deste repositório)`: arquivos modificados/adaptados em 2026-11-06; gerência, saída, foco e consumidores Escape adaptados aos contratos Donna, sem tradução/promoção/afterExit upstream | [Apache-2.0](https://github.com/leepokai/Codync/blob/219935410dffccb950f90d0565bd1d9f1661bd5b/LICENSE) | 2026-11-06 |
| [Codync](https://github.com/leepokai/Codync/commit/c785c49e98c4e9b5ec523d59d6bd9e36eb274215) — Kai Wu; coautorias históricas: leepokai <109857817+leepokai@users.noreply.github.com> e Claude Opus 5.5 <noreply@anthropic.com> (metadados conferidos via gh api) | `c785c49e98c4e9b5ec523d59d6bd9e36eb274215` | `apps/desktop/src/renderer/views/thread/attachments.css`: duas trilhas `minmax(0, 1fr)` | `apps/desktop/src/renderer/views/thread/attachments.css` (modificado em 2026-11-06): somente restrição da grade; TSX e aumento upstream não portados. Documentação e fixture de QA próprias da Mexerica, sem cópia de texto upstream | [Apache-2.0](https://github.com/leepokai/Codync/blob/c785c49e98c4e9b5ec523d59d6bd9e36eb274215/LICENSE) | 2026-11-06 |
| [Codync](https://github.com/leepokai/Codync) — leepokai <109857817+leepokai@users.noreply.github.com>; coautoria histórica Claude Opus 5.5 <noreply@anthropic.com> (ambos os commits; metadados e LICENSE conferidos via API GitHub) | `57537a1e5d52f27c595af7415cc4f69a79dff080`, `bb242a7df4a9479fac622657a70f4007cb690fe3` | `host/src/service/addresses.rs` (filtro por prefixo e teto 16 do primeiro; somente prioridade LAN do segundo); `host/src/service/mod.rs` (extração do primeiro) | `host/src/fork/pairing_addresses.rs` (porte seletivo adaptado, sem URL externa configurável), `host/src/fork/mod.rs`, `host/src/service/mod.rs` (ganchos); testes sintéticos próprios em `host/src/fork/pairing_addresses_tests.rs`. Demais assuntos dos commits, apps e textos do site upstream não portados | [Apache-2.0](https://github.com/leepokai/Codync/blob/57537a1e5d52f27c595af7415cc4f69a79dff080/LICENSE), [LICENSE do segundo pin](https://github.com/leepokai/Codync/blob/bb242a7df4a9479fac622657a70f4007cb690fe3/LICENSE) | 2026-11-06 |
| [ai-memory](https://github.com/akitaonrails/ai-memory) | `a295983828901aa612d37dabd6045500346d457a` | `crates/ai-memory-core/src/sanitize.rs` (padrões de redação) | `host/crates/memory/src/redact.rs` | MIT (aviso abaixo) | 2026-10-20 |
| [ai-memory](https://github.com/akitaonrails/ai-memory) | `a295983828901aa612d37dabd6045500346d457a` | `crates/ai-memory-core/src/workstream.rs` (`UNTRUSTED_MEMORY_NOTICE`, texto adaptado) | `host/crates/memory/src/opening.rs`, `host/src/agent/harness.rs` | MIT (aviso abaixo) | 2026-10-20 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/lib/routine-calendar.ts` (datas, encaixe, colunas de sobreposição, rótulo GMT); `src/components/routines/MiniMonth.tsx` (grade de 42 dias) | `apps/desktop/src/renderer/lib/routine-calendar.ts` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/components/routines/MiniMonth.tsx` | `apps/desktop/src/renderer/views/rotinas-central/MiniMonth.tsx` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/components/routines/CalendarSidebar.tsx` | `apps/desktop/src/renderer/views/rotinas-central/BotsPanel.tsx` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/components/RoutineCalendarPage.tsx` (`CalendarGrid`, `CalendarEventCard`) | `apps/desktop/src/renderer/views/rotinas-central/TimeGrid.tsx`, `routines-central.css` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/components/automations/AutomationsCalendar.tsx` | `apps/desktop/src/renderer/views/rotinas-central/RoutineCalendar.tsx` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/components/TriggersPage.tsx` (cabeçalho) | `apps/desktop/src/renderer/views/rotinas-central/RoutinesCentralView.tsx` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/locales/pt-br.json` (chaves `routines.*` e `automations.*`, textos adaptados) | `apps/desktop/src/renderer/views/rotinas-central/texts.ts` | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [Anomalia](https://github.com/anomaliaso/anomalia) | `6a53dff4eea44f4c543880dba69e673fdcf2c430` | `packages/agent-contracts/src/specs.ts` (bloco `COMMON` e a instrução do agente de conteúdo: regras de conduta comuns da equipe) | `apps/desktop/src/renderer/views/fork/times/modelo-marketing.ts` (`REGRAS_MARKETING`, cinco regras adaptadas em PT-BR, tradução ou paráfrase próxima) | Apache-2.0 (aviso abaixo) | 2026-11-06 |
| [Lucide](https://github.com/lucide-icons/lucide) (pacote `lucide` 1.52.0, já dependência do desktop) | — (versão do pacote) | desenhos SVG de ícones (`search`, `inbox`, `users`, `calendar-clock`, `settings` e os demais do objeto `P`), transcritos | `redesenho/components/bundle.js` e propostas `times-20261103/`, `times-02-20261103/` e `redesenho/propostas/contas-por-motor-20261115/` (`gerar.mjs` e os HTML gerados em `html/`; PNGs derivados — arquivados em [mexerica-design-archive]((arquivo de design privado do projeto de origem, fora deste repositório))); `docs/propostas/marketplace-20261116/` (`gerar.mjs`, HTML e PNGs derivados; ícones dos previews; no app seguem SF Symbols e `icon-fallbacks.ts`) | ISC; ícones derivados do Feather sob MIT (aviso abaixo) | 2026-11-03 |
| [security-audit-skill](https://github.com/cloudflare/security-audit-skill) — Cloudflare | `c1c8a8c1471069fb0e188eeaff69b8e8db6564a8` | `skills/security-audit/` (todos os arquivos) | `.claude/skills/security-audit/` (cópia sem alteração) | MIT (aviso abaixo) | 2026-11-07 |
| [tstack](https://github.com/t3dotgg/tstack) — Theo Browne | `1085050f3baed1ac724caa4a8166145fb2ac88af` | `skills/babysit-pr/scripts/pr-wait` | `.claude/skills/acompanhar-pr/scripts/pr-wait` (cópia sem alteração) | MIT (aviso abaixo) | 2026-11-04 |
| [tstack](https://github.com/t3dotgg/tstack) — Theo Browne | `1085050f3baed1ac724caa4a8166145fb2ac88af` | `skills/sol-loop`, `babysit-pr`, `file-pr`, `full-send`, `full-stop` e `leaving-pr-comments` (`SKILL.md`, texto adaptado) | `AGENTS.md` (Rodadas de revisão, escopo e PR), `.claude/skills/{abrir-pr,acompanhar-pr,full-stop}/SKILL.md`, `.github/pull_request_template.md` (reescritos em PT-BR para as regras da Mexerica) | MIT (aviso abaixo) | 2026-11-04 |
| [wacli](https://github.com/openclaw/wacli) — Peter Steinberger (v0.25.1) | `b7603b697e69be591afa83379ced1e73177604c6` | `internal/store/schema.sql` (tabelas `chats`, `contacts`, `messages`, `call_events` e índices `idx_call_events_*`) | `host/src/fork/whatsapp/wacli_fixture.sql` (cópia sem alteração; só fixture de teste do R3/R5c do WhatsApp) | MIT (aviso abaixo) | 2026-11-05 |
| [emilkowalski/skills](https://github.com/emilkowalski/skills) — Emil Kowalski | `5206a950b715e0cabe7043ea0b2304332b91494e` (main, conferido em 2026-11-08; pin da cópia original não registrado) | `skills/<nome>/SKILL.md` das 13 skills listadas na seção abaixo | `.claude/skills/<nome>/SKILL.md` (cópia sem alteração local; ver seção para o estado de cada uma) | MIT (aviso abaixo; cópia integral em `.claude/skills/LICENSE-MIT-emilkowalski.txt`) | 2026-11-08 |
| [pi-mono](https://github.com/badlogic/pi-mono) — Mario Zechner | `22fc026b872b4a7df22ffa94e8fa00b873128054` | `packages/schema/src`, `packages/telemetry/src`, `packages/agent/src`, subconjunto de `packages/ai/src` (sem catálogos gerados nem `compat`), `packages/coding-agent/src/modes/rpc/jsonl.ts` | `motor/vendor/pi-mono/packages/{schema,telemetry,ai,agent,coding-agent}/` (cópia com ganchos curtos listados em `motor/vendor/pi-mono/PATCHES.md`; `LICENSE` MIT preservado na pasta raiz da cópia) | MIT (aviso abaixo) | 2026-11-16 |
| [@anthropic-ai/sdk](https://www.npmjs.com/package/@anthropic-ai/sdk) — Anthropic | `0.129.0` (versão fixa no `motor/package-lock.json`) | SDK oficial da Anthropic, dependência do pi-ai vendorizado | `motor/node_modules` (instalação; declaração fixa em `motor/vendor/pi-mono/packages/ai/package.json` e `motor/package-lock.json`) | MIT | 2026-11-16 |
| [openai](https://www.npmjs.com/package/openai) — OpenAI | `7.19.0` (versão fixa no `motor/package-lock.json`) | SDK oficial da OpenAI, dependência do pi-ai vendorizado | `motor/node_modules` (instalação; declaração fixa em `motor/vendor/pi-mono/packages/ai/package.json` e `motor/package-lock.json`) | Apache-2.0 | 2026-11-16 |
| [@google/genai](https://www.npmjs.com/package/@google/genai) — Google | `2.32.0` (versão fixa no `motor/package-lock.json`) | SDK oficial do Google, dependência do pi-ai vendorizado | `motor/node_modules` (instalação; declaração fixa em `motor/vendor/pi-mono/packages/ai/package.json` e `motor/package-lock.json`) | Apache-2.0 | 2026-11-16 |
| [React](https://github.com/facebook/react) — Meta (Facebook, Inc. and its affiliates) | `react`/`react-dom`/`scheduler` 18.3.1 (bundle esbuild do demo) | `react/cjs/react.production.min.js`, `react/cjs/react-jsx-runtime.production.min.js`, `react-dom/cjs/react-dom.production.min.js`, `scheduler/cjs/scheduler.production.min.js` | `apps/desktop/src/renderer/lib/donnabot-avatars/demo/demo.js` (bundle minificado do demo; avisos MIT embutidos no bloco "Bundled license information" do próprio arquivo) | MIT | 2026-11-17 |


Antes de distribuir ou entrar em uso de empresa, revise as licenças registradas.
Código e texto copiados não devem ser publicados na wiki do produto.

## Avisos de licença

### ai-memory

```text
MIT License

Copyright (c) 2026 Fabio Akita

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### security-audit-skill

```text
MIT License

Copyright (c) 2025-2026 Cloudflare, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### tstack

```text
MIT License

Copyright (c) 2026 Theo Browne

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### wacli

```text
MIT License

Copyright (c) 2026 Peter Steinberger

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### emilkowalski/skills — 13 skills MIT

Origem: <https://github.com/emilkowalski/skills> (MIT, Copyright (c) 2026 Emil
Kowalski). Cópia integral da licença em
`.claude/skills/LICENSE-MIT-emilkowalski.txt`,
ao lado das pastas. O `skills-lock.json` da raiz registra origem e hash de
instalação. Conferência de 2026-11-08 contra a `main` upstream
(`5206a950b715e0cabe7043ea0b2304332b91494e`): cópias sem alteração local; as
três primeiras da lista abaixo conservam o texto de uma revisão anterior do
upstream (antes da atualização sobre aceleração de hardware do Motion 14.2).

| Skill | Estado da cópia |
|---|---|
| `animate` | revisão anterior do upstream (Motion 14.2) |
| `emil-design-eng` | revisão anterior do upstream (Motion 14.2) |
| `review-animations` | revisão anterior do upstream (Motion 14.2) |
| `animate-expo` | idêntica à main upstream |
| `animation-vocabulary` | idêntica à main upstream |
| `apple-design` | idêntica à main upstream |
| `ask-sonner` | idêntica à main upstream |
| `find-animation-opportunities` | idêntica à main upstream |
| `improve-animations` | idêntica à main upstream |
| `mobile-native` | idêntica à main upstream |
| `pick-ui-library` | idêntica à main upstream |
| `prototype` | idêntica à main upstream |
| `write-swift` | idêntica à main upstream |

Aviso aplicável:

```text
MIT License

Copyright (c) 2026 Emil Kowalski

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### OpenMausBot

Apache License 2.0 (<https://www.apache.org/licenses/LICENSE-2.0>), o mesmo texto
de `LICENSE` deste repositório. Os arquivos adaptados levam no topo a origem e a
nota de alteração. Linha do `NOTICE` do OpenMausBot:

```text
OpenMausBot
Copyright 2026 Milind Soni and OpenMausBot contributors
```

Em 16/11/2026 um levantamento de concorrente copiou 47 documentos `docs/*.md`
de topo do commit `49968d7f12f381ac38f33d9318208c04c7afef17`; essas cópias não
estão mais no repositório — vivem nas Notes privadas do dono. O `docs/` de topo
tem 56 `.md`: 9 ficaram só como links porque contêm exemplos de IP privado,
caminho de serviço ou chave de API de exemplo que disparam o scanner de
vazamento ou o Gitleaks do CI (inclui `meeting-notes.md`). As subpastas de
`docs/` não foram cobertas.
Os nomes OpenMausBot/MausBot e a mascote são marcas de Supamaus Software Private Limited, não licenciadas
pela Apache-2.0 (`LICENSING.md` do upstream). A pasta `enterprise/` do upstream tem exceção restritiva de
licença registrada no épico de Times (`docs/plans/pessoal-teams-epico-20261109.md`, pin `e164d342`).

### Anomalia

Apache License 2.0 (<https://www.apache.org/licenses/LICENSE-2.0>), o mesmo texto
de `LICENSE` deste repositório. O repositório não tem `NOTICE` no commit citado.
`modelo-marketing.ts` leva a origem no topo; `REGRAS_MARKETING` é adaptação
(tradução para PT-BR) do bloco `COMMON` citado na tabela.

### SF Symbols (Apple) — gerados do sistema, nunca versionados

Nenhum símbolo, imagem ou fonte do SF Symbols está neste repositório. No macOS, o build executa
`apps/desktop/tools/export-symbols.swift`, que lê os nomes de `apps/desktop/src/renderer/components/symbols.json`
e desenha cada símbolo a partir do AppKit do próprio sistema em máscaras PNG, gravadas em
`src/renderer/public/symbols/` (ignorado pelo Git) e embutidas só no app para plataformas Apple. Linux e Windows usam os
ícones Lucide (abaixo), sem SF Symbols.

- **Origem:** SF Symbols da Apple, via AppKit, no Mac de quem compila.
- **Condição de uso registrada no código:** segundo o comentário do exportador, SF Symbols só podem ser distribuídos em apps
  para plataformas Apple; por isso as máscaras são geradas no build e não são versionadas. Este repositório não traz o texto da
  licença da Apple: quem distribuir um instalador de Mac deve conferir os termos vigentes do SF Symbols.
- **Logotipo da Apple:** o símbolo `apple.logo` **não** é exportado (marca de terceiros). O botão do provedor Apple usa
  `provedor.apple`, ícone genérico de conta da Lucide. `tools/verify-build.cjs` recusa o pacote de macOS se `apple.logo` voltar
  ao manifesto exportado.

### Lucide

```text
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The following Lucide icons are derived from the Feather project:

airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out

The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```


## Fontes da interface — Barra04 (2026-11-05)

Ajustes → Aparência → Fonte (padrão DM Sans). Os arquivos vão embutidos no app, em
`apps/desktop/src/renderer/assets/fonts/`, e nunca são baixados em execução. São as versões variáveis
(peso 100–900), só o subconjunto latino, como publicadas pelo [Fontsource](https://fontsource.org) 5.3.0
(`@fontsource-variable/<fonte>`, baixadas com `npm pack`; nenhuma dependência nova no projeto). Sem alteração
nos arquivos. Licença: SIL Open Font License 1.1, com o texto completo ao lado de cada arquivo (`OFL-<fonte>.txt`).

| Fonte | Arquivo | Origem | Copyright |
| --- | --- | --- | --- |
| DM Sans | `dm-sans-latin-wght-normal.woff2` | https://github.com/googlefonts/dm-fonts | Copyright 2014 The DM Sans Project Authors |
| Inter | `inter-latin-wght-normal.woff2` | https://github.com/rsms/inter | Copyright 2016 The Inter Project Authors |
| Geist | `geist-latin-wght-normal.woff2` | https://github.com/vercel/geist-font | Copyright 2024 The Geist Project Authors |
| Manrope | `manrope-latin-wght-normal.woff2` | https://github.com/sharanda/manrope | Copyright 2019 The Manrope Project Authors |
| IBM Plex Sans | `ibm-plex-sans-latin-wght-normal.woff2` | https://github.com/IBM/plex | Copyright 2019 IBM Corp. |

## Marcas de serviços do catálogo de conectores

Esta cópia **não distribui** logos de serviços de terceiros. Todo serviço do catálogo de conectores aparece com o globo neutro
(`views/marketplace/marcas.ts` não mapeia nenhum arquivo). Marcas e logos pertencem a seus titulares e não são cobertas pela
licença Apache-2.0 deste repositório. Um logo só pode ser adicionado com comprovação do direito de uso; ver também [TRADEMARK.md](TRADEMARK.md).

### Fontes e sons da skill product-launch-film (2026-11-08)

A skill `.claude/skills/product-launch-film/` (texto e template) é herança do upstream
Codync (commit `4f58acb92feaa5899ccd7da435e6c21a3e21aaa5`, "docs: add a product launch
film skill"), coberta pela Apache-2.0 da base já atribuída em LICENSE/NOTICE. Seus ativos:

- **Fontes Geist** (`geist.woff2`, `geist-mono.woff2`, `geist-mono-sym.woff2` em
  `template/assets/fonts/`): do projeto [Vercel geist-font](https://github.com/vercel/geist-font),
  **SIL Open Font License 1.1**, Copyright 2024 The Geist Project Authors. O upstream não
  incluía a licença; o texto integral OFL 1.1 foi acrescentado junto dos arquivos em
  `template/assets/fonts/OFL-geist.txt` (mesma cópia usada pelo app em
  `apps/desktop/src/renderer/assets/fonts/OFL-geist.txt`). Sem alteração nos binários.
- **Sons** (`template/assets/sfx/`): os 8 MP3 herdados do upstream (pop-a..d, pop,
  click-soft, whoosh, chime) não tinham procedência nem licença verificáveis — o commit
  upstream que os adicionou não declara origem ou licença dos arquivos de áudio, e não há
  registro em arquivos da skill. **Removidos** em 2026-11-08; o template foi ajustado para
  renderizar silencioso e a skill orienta usar somente áudio licenciado ou gerado pelo
  próprio autor. Nenhum som foi baixado para repor.

## Avatares próprios da Mexerica — D115

`@donnabot/avatars` foi fornecida pelo dono do projeto em 2026-10-21 como código
próprio da Mexerica, sem repositório/commit de terceiro. A cópia está em
`apps/desktop/src/renderer/lib/donnabot-avatars/`: README, guia de integração,
`src/`, `presets/`, `schema/` e `demo/`. Nenhum arquivo de `src/` foi alterado.
A cópia exclui `.DS_Store` e `node_modules`. Não se atribui licença de terceiro
à biblioteca própria. Os adaptadores ficam fora dela, em `renderer/fork/avatars/`.
O desenhista de personagens canvas herdado do desktop e seu seletor foram removidos;
a compatibilidade de nomes de campos antigos não reutiliza os desenhos upstream.
Os clientes móveis e seus recursos continuam aguardando a adoção posterior da D115.

A atribuição ao Codync permanece em `LICENSE` e `NOTICE`.

## Catálogo passivo D166 — reparo LANG-1 (2026-10-25)

`host/src/fork/skill_catalog/registry.json` preserva as mesmas 26 entradas e seus repositórios/pins/origens
aceitos no #77. A correção restaura nome/descrição/identificador e os bytes originais de
SKILL.md nos 11 recortes importáveis. Não atualiza upstream nem altera pacotes instalados.
A revisão local `donna-original-text-v2` identifica o reparo de embalagem e suas omissões;
novo envelope/digest, sem alegar hash equivalente à embalagem anterior.

Inventário por arquivo: `host/src/fork/skill_catalog/source-manifest.json`, com origem,
Git blob, SHA-256, bytes, linhas, licença efetiva, evidência da licença, destino, omissões e
alterações (nenhuma nos textos originais; aviso de embalagem próprio separado). Os textos
integrais de LICENSE/NOTICE/CREDITS aplicáveis estão nos assets dos respectivos pacotes.
`apps/desktop/src/renderer/demo/fake-skills.ts` consome esse mesmo bundle para preservar
os exemplos no idioma original, com estado/recibos sintéticos; nenhuma cópia traduzida.
Não há dependência nova nem modificação de lockfile, fonte, imagem, ícone ou runtime.

| Origem / créditos nos avisos | Pin | Recorte atual / licença efetiva |
|---|---|---|
| cloudflare/security-audit-skill — Cloudflare, Inc. 2025–2026 | c1c8a8c1471069fb0e188eeaff69b8e8db6564a8 | SKILL.md, referências textuais e esquema JSON originais; MIT, LICENSE integral |
| pbakaus/impeccable | 936867543ac2576920d663cc9803597f6044f3ac | SKILL.md original; Apache-2.0, LICENSE e NOTICE.md integrais; referências derivadas de ehmo não copiadas, sem declarar o aviso MIT como licença do conjunto |
| anthropics/skills | dbd4588f9e1033efb41dad4bef2f7947c8993d44 | frontend-design textual original, Apache-2.0 específico de skills/frontend-design/LICENSE.txt; demais condições abaixo |
| vercel-labs/skills — Vercel, Inc. 2026 | a80e4c9d12d607631249d4d7d059f835d4b2f13a | find-skills original, MIT, LICENSE integral; CLI não distribuído |
| mattpocock/skills — Matt Pocock 2026 | 5beb35b040e3de97e54fa9e386d7e42d15c295c2 | grilling e research originais, MIT, LICENSE integral; handoff indisponível |
| obra/superpowers — Jesse Vincent 2025 | 2ce8aa15d05bd57213e6f746f953aafb7ded7a8b | brainstorming original, MIT, LICENSE integral; companion/scripts não distribuídos |
| coreyhaines31/marketingskills — Corey Haines 2025 | e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b | content-strategy e marketing-psychology preservados, MIT, LICENSE integral; copywriting adaptada na D353 (abaixo) |
| heygen-com/hyperframes | 9aadc0b9b99d92afd25215228878cee23ea47a0f | SKILL.md e referências Markdown originais, Apache-2.0, LICENSE/CREDITS.md integrais; fontes/pesos/SDK/CLI/Studio/assets gráficos não copiados |
| davidondrej/skills — David Ondrej 2026 | 8605863f211961c96208fe625d4024609cd7dbd5 | Boat e references/operations.md originais, MIT, LICENSE integral; nenhuma execução/provisão |

PDF/XLSX permanecem apenas metadados sob LicenseRef-Anthropic-Proprietary; nenhum corpo
proprietário no produto. A D353 exclui o auxiliar natural-transitions não
verificado e libera somente a adaptação declarada de copywriting (abaixo). Orca/stablyai c90d0f93c0f2f2c3ec243046c36059a1f4ec5cd2 é referência MIT sem corpo
ou runtime distribuídos. Os onze novos bloqueios são de compatibilidade, não de licença:
Hallmark/Nutlope 7ec868225c6caa040558c321ee0fefa0e73f83a8 (MIT, Hallmark contributors 2026),
skill-creator/Anthropic (Apache-2.0 específico), handoff/Matt Pocock (MIT),
Browser Use 3df5806a0d097a1546d92c49bcf1b28c9972c3bd (MIT, Gregor Zunic 2024) e sete guias
Firecrawl (ISC). Nomes/descrições originais permanecem; nenhum YAML é reescrito.

Firecrawl mantém as fontes textuais precisas: firecrawl/skills@2bf714d9f45e85085036a533ceb943c8577aef2b
(build-scrape/search/interact e skills/core/firecrawl/search/scrape) e
firecrawl/firecrawl-workflows@d2a986240f9d786ae54fc33fcfd9ad37308e9cc3 (deep-research).
O corpo não é distribuído nesta rodada; a licença do runtime firecrawl/cli@2f2825d7788725967f11607761818b2565314dc7
continua em revisão, sem trocar a procedência textual para esse repo por inferência.

`legacy-envelopes.json` preserva metadados/envelopes históricos sem corpos e apenas para
reconhecer recibos exatos. `legacy-fixtures.json` contém duas embalagens antigas de teste
(find-skills MIT e frontend-design Apache-2.0), com licenças completas preservadas nos assets;
não são instaláveis pelo catálogo. Elas comprovam ausência de regravação/renomeação/upgrade.
O índice central de pesquisa será consolidado pelo coordenador, sem edição concorrente.
Registro e crédito não autorizam ferramentas, custos, serviços ou redistribuição de runtimes.
## Spaces — D169

O módulo `host/src/fork/spaces/`, o controle contextual em
`host/src/agent/bot/space_control.rs`, os DTOs/wrappers
`apps/desktop/src/shared/fork-spaces*` e a fixture
`host/tests/fixtures/spaces_agent.py` foram escritos para a Mexerica.
Os ganchos modificam a base Codync v2.20.3, commit
`c7548b2896ba40192973760015be9d220dab95e7`, já atribuída em LICENSE/NOTICE.
`aecb03805663ace8d3e9403bff52f75893f1ce62` identifica a tag anotada, não o commit.

Os IDs semânticos de ícones/aparência seguem o material do design system já
versionado em `docs/reference/redesenho/components/SpaceIconPicker/README.md` e
`docs/reference/design/redesenho/personalizacao.md` na base Mexerica `cd9893f1`.
Esta autoria de backend não copiou desenhos, fontes, fotos ou wallpapers.
Os payloads PNG/SVG dos testes são fixtures sintéticas. A licença dos ativos
visuais permanece no registro próprio de sua origem; estes IDs não a substituem.

`spaces_e2e.rs` reutiliza uma cópia temporária da fixture própria
`host/tests/fixtures/engine_setup_cli.py` da base `cd9893f1`, acrescentando um
modelo de teste ao catálogo temporário; o arquivo original foi preservado.
Não houve nova dependência nem alteração de versões/lockfiles nesta fatia.
Este registro descreve um candidato D169, não incorporação, instalação,
redistribuição ou aprovação de uso comercial.

### Correção autoral D169 R2

As sondas `host/tests/review_spaces_r1.rs`, `host/tests/fixtures/review_spaces_agent_r1.py` e `apps/desktop/review-r1.cjs` vêm da revisão independente first-party D169 R1 de 24/10/2026, preservada em `research/reports/evidencias/d169-spaces-review-r1/instrumentation-final` do repositório privado de pesquisa. Foram incorporadas como regressões do candidato e ampliadas nesta autoria para seletores legados, geração BotSync, manifesto adulterado, dimensões SVG e retomada adicional/ABA/reinício. A fixture continua mantendo índice externo por raw-SID/cwd e registrando o contexto consumido. O helper `agent/bot/waiting/spaces.rs` e a ponte de teste desktop são código original do fork. Não há nova dependência, lockfile ou ativo visual; LICENSE/NOTICE e atribuições anteriores permanecem. Estado: candidato de autoria, sem adoção/instalação inferida.


### Fronteira PTY Claude — D169 R3

A alteração em `host/src/fork/engine_setup/process.rs` e as sondas correlacionadas
em `host/src/fork/engine_setup/tests.rs` são autoria própria da Mexerica.
A fixture `host/tests/fixtures/engine_setup_cli.py` estende a fixture first-party
D164 já presente no candidato `3d1126f399a34fcb36db12eaa3b88ca4bfd3a540`.
O controle de fechamento antecipado de descritores foi motivado pela evidência
first-party da revisão D169 R2 de 24/10/2026, preservada em
`research/reports/evidencias/d169-spaces-review-r2/pty-eof-exact-instrumentation.patch`
no repositório privado de pesquisa. Esta autoria usa liberação explícita em arquivo
descartável para controlar o exit, com sondas de deadline, cancelamento, status
final e ausência de relançamento. Não incorpora fonte de terceiro, dependência,
ativo ou texto de skill. LICENSE/NOTICE e atribuições existentes permanecem.
Estado: candidato de correção, sem merge, instalação ou redistribuição inferidos.


### Prontidão de processos — D169 lifecycle R4

`host/src/agent/bot/lifecycle_fixture.rs`,
`host/tests/fixtures/lifecycle_process.pl` e os testes correlatos de
`host/src/agent/bot/lifecycle.rs` são autoria first-party da Mexerica nesta
rodada. Partem do teste próprio já presente no candidato
`940cd3847b236d5660e7d1f836afb029e03dc11d`, substituindo a espera fixa por
handshake de fork/session/descendentes e encerramento/reap explícitos.
A motivação é o bloqueio R3-GATE-01, registrado na revisão independente
D169 R3 de24/10/2026; o controle sintético não reconstrói a ocorrência histórica.
Não incorporam fonte, texto de skill, dependência ou ativo externo; usam Perl,
POSIX e bibliotecas já presentes. LICENSE/NOTICE e atribuições existentes
permanecem. Estado: candidato de correção, sem merge/instalação/redistribuição.

### Limpeza da fixture lifecycle — D169 R5

Correção first-party de `host/src/agent/bot/lifecycle_fixture.rs`,
`host/tests/fixtures/lifecycle_process.pl` e suas regressões, sobre
`847d06a67743d541f810848024b31621f6f3f86b`, motivada por R4-F1 da revisão
independente D169 R4 de24/10/2026. O controle nativo first-party dessa revisão
é a origem da regressão de morte do intermediário após prontidão; as provas
estão no repositório privado de pesquisa, `research/reports/evidencias/d169-spaces-review-r4/`.
A supervisão e o recolhimento de órfãos ficam no processo sintético separado;
nenhuma configuração de subreaper é aplicada ao Hub ou ao runner de testes.
Não incorporam código externo, dependências, ativos ou texto de skill, nem
alteram LICENSE/NOTICE. Estado: candidato autoral, sujeito a nova revisão
independente; sem merge, instalação ou redistribuição.


### Posse da fixture lifecycle — D169 R6

Correção first-party sobre `e6fe4819ed2108bdcb69f926df5c7ced3883d536` em
`host/tests/fixtures/lifecycle_process.pl` e controle focal em
`host/src/agent/bot/lifecycle_fixture.rs`, motivados por R5-F1 da revisão
independente D169 R5 de25/10/2026. Os drivers nativos first-party dessa revisão
foram reutilizados apenas nas evidências privadas de pesquisa; o controle
incorporado observa a sinalização e o recolhimento de filhos sintéticos e
exercita interrupções na fronteira de posse. Nenhum código externo, dependência,
ativo ou texto de skill foi incorporado. Supervisão/subreaper continuam apenas
na fixture; LICENSE/NOTICE permanecem. Estado: candidato autoral para nova
revisão independente, sem merge, instalação ou redistribuição.


### ChaCha20-Poly1305 do cliente Electron

Dependência de produção `@noble/ciphers` **2.4.0**, sem adaptações, importada de
`@noble/ciphers/chacha.js` por `apps/desktop/src/main/fork-cloud/crypto.ts`.
Origem: <https://github.com/paulmillr/noble-ciphers/tree/2.4.0>; tarball e
integridade em `apps/desktop/package-lock.json`. A fixture Cloud já trava a mesma
versão em seu lock. MIT conferida no pacote e idêntica à tag; SHA256 do LICENSE:
`f36671a5487c9c5050efacb58011c37c24c55a889803cb036cf9d9a6347c1e2d`. O pacote de produção conserva seu LICENSE;
conferir também o artefato distribuído. Estado: candidato em PR, sem instalação.

```text
The MIT License (MIT)

Copyright (c) 2022 Paul Miller (https://paulmillr.com)
Copyright (c) 2016 Thomas Pornin <pornin@bolet.org>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the “Software”), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

### Porte upstream-01 — consulta Claude sem MCP

Adaptação de 03/11/2026: `host/src/usage.rs` preserva ambiente filtrado, slot e
perfil existentes, `--setting-sources ""`, persistência desativada, prazo, stdin
fechado, kill_on_drop e parser da Donna. Somente as flags MCP foram aproveitadas
do arquivo upstream registrado na tabela. Autoria upstream: Kika
; o commit original não contém trailer de coautoria.
`host/src/fork/usage_probe.rs`, seu registro de testes em `host/src/fork/mod.rs`
e `host/tests/fixtures/upstream_usage_cli.py` são testes próprios do fork, não
cópias do teste upstream de constante de flags. O pequeno ponto de injeção de
executável/pasta/prazo em `usage.rs` permite testar a chamada real sem mutação
global concorrente do ambiente. Documentação própria modificada:
`docs/features/setup-provedores.md`, `docs/features/manual-do-harness.md` e
`docs/plans/upstream-lote1-20261103.md`; este registro também foi modificado.
LICENSE e NOTICE permanecem integrais, com Po Kai Lee e contribuidores. Não há
nova dependência, ativo ou corpo de skill; fixture não comprova o CLI real.

## Marketing passivo — D353 / DON-902 (06/11/2026)

Os assets ficam em `host/src/fork/skill_catalog/registry.json`; manifesto de
procedência por arquivo em `source-manifest.json` no mesmo diretório. Cada
pacote leva LICENSE integral e ADAPTATION próprio em PT-BR. Os textos originais
continuam em inglês; os suplementos próprios ao SKILL também usam inglês para
conservar o idioma da orientação. LICENSE/NOTICE base não foram alterados.

| Pacote / arquivo distribuído | Fonte original no pin | Crédito / licença | Alteração |
| --- | --- | --- | --- |
| `copywriting/ADAPTATION.md` | autoria própria D353 | Mexerica — MIT com o pacote | autoria própria Mexerica D353 |
| `copywriting/LICENSE` | [LICENSE](https://github.com/coreyhaines31/marketingskills/blob/e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b/LICENSE) | © 2025 Corey Haines — MIT | nenhuma; bytes originais |
| `copywriting/LICENSE-Profound.txt` | [LICENSE](https://github.com/retieedra-profound/skills-marketing-library/blob/80293f156c27d3f90aedebfb3c27309769b2eec6/LICENSE) | © 2026 Profound — MIT | nenhuma; bytes originais |
| `copywriting/SKILL.md` | [skills/copywriting/SKILL.md](https://github.com/coreyhaines31/marketingskills/blob/e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b/skills/copywriting/SKILL.md) | © 2025 Corey Haines — MIT | adaptação textual declarada em ADAPTATION.md |
| `copywriting/references/ai-tells.md` | [skills/copywriting/references/ai-tells.md](https://github.com/coreyhaines31/marketingskills/blob/e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b/skills/copywriting/references/ai-tells.md) | © 2025 Corey Haines — MIT | nenhuma; bytes originais |
| `copywriting/references/copy-frameworks.md` | [skills/copywriting/references/copy-frameworks.md](https://github.com/coreyhaines31/marketingskills/blob/e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b/skills/copywriting/references/copy-frameworks.md) | © 2025 Corey Haines — MIT | nenhuma; bytes originais |
| `stop-slop/ADAPTATION.md` | autoria própria D353 | Mexerica — MIT com o pacote | autoria própria Mexerica D353 |
| `stop-slop/LICENSE` | [LICENSE](https://github.com/hardikpandya/stop-slop/blob/8da1f030185bdfe8471220585162991eaeb970e9/LICENSE) | © 2025 Hardik Pandya — MIT | nenhuma; bytes originais |
| `stop-slop/SKILL.md` | [SKILL.md](https://github.com/hardikpandya/stop-slop/blob/8da1f030185bdfe8471220585162991eaeb970e9/SKILL.md) | © 2025 Hardik Pandya — MIT | nenhuma; bytes originais |
| `stop-slop/references/examples.md` | [references/examples.md](https://github.com/hardikpandya/stop-slop/blob/8da1f030185bdfe8471220585162991eaeb970e9/references/examples.md) | © 2025 Hardik Pandya — MIT | nenhuma; bytes originais |
| `stop-slop/references/phrases.md` | [references/phrases.md](https://github.com/hardikpandya/stop-slop/blob/8da1f030185bdfe8471220585162991eaeb970e9/references/phrases.md) | © 2025 Hardik Pandya — MIT | nenhuma; bytes originais |
| `stop-slop/references/structures.md` | [references/structures.md](https://github.com/hardikpandya/stop-slop/blob/8da1f030185bdfe8471220585162991eaeb970e9/references/structures.md) | © 2025 Hardik Pandya — MIT | nenhuma; bytes originais |
| `copy-review-panel/ADAPTATION.md` | autoria própria D353 | Mexerica — MIT com o pacote | autoria própria Mexerica D353 |
| `copy-review-panel/LICENSE` | [LICENSE](https://github.com/Yuzzyuk/marketing-os/blob/bb67dff5f04b390e861ee11433166e4519e7f4c0/LICENSE) | © 2026 Marketing OS contributors — MIT | nenhuma; bytes originais |
| `copy-review-panel/SKILL.md` | [skills/marketing-os/references/copy.md](https://github.com/Yuzzyuk/marketing-os/blob/bb67dff5f04b390e861ee11433166e4519e7f4c0/skills/marketing-os/references/copy.md) | © 2026 Marketing OS contributors — MIT | adaptação textual declarada em ADAPTATION.md |
| `copywriting/references/donna-transitions.md` | autoria própria D353; sem fonte upstream | Mexerica — MIT com a adaptação | guia original em inglês; não derivado do auxiliar excluído |

Copywriting mantém o pin e sourceDigest original do produto. O auxiliar
`natural-transitions.md` não foi incorporado nem usado para escrever outro
texto. Seu link no SKILL foi trocado para `references/donna-transitions.md`, guia curto
autoral independente da Mexerica D353 em inglês; evals também ficam fora.
Além da troca do link, a adição ao SKILL é a seção final de uso passivo,
identificada no aviso.
Referências ai-tells/copy-frameworks e LICENSE permanecem byteiguais. As fontes
citadas internamente em ai-tells são descritas pelo autor como síntese em suas
próprias palavras; os créditos internos permanecem. O aviso MIT Profound foi
conferido separadamente no pin da tabela e acompanha a adaptação já declarada
pelo autor, sem importar mais skills. Números/exemplos externos não foram
validados como evidência de resultados e não autorizam alegações do usuário.

Stop-slop conserva SKILL, cabeçalho metadata e três referências byteiguais;
README com imagem e CHANGELOG omitidos. O parser desta base já aceita metadata
de textos: nenhuma modificação de parser ou dos cabeçalhos foi necessária.

Copy-review-panel é nome/descrição da adaptação, derivada apenas do método
textual MIT de copy.md; a tabela é byteigual e o restante é adaptação/autoria
local declarada. Cinco notas 0–20, três rodadas e corte 70 são conferidos no
texto MIT. O corte 85 para apresentar ao dono é a decisão D353; a fonte sugere
parar em 90+ e não determina publicação em 85. Não se copiou copy-panel.ts.

SourceDigest das duas origens novas: SHA-256 do JSON UTF-8 compacto, chaves
ordenadas, `{path: sha256}` dos arquivos de origem efetivamente usados neste
recorte, incluindo LICENSE. Mapa e método estão no source-manifest. O envelope
canônico existente e seus hashes por asset identificam separadamente os bytes
adaptados, incluindo textos autorais. Não há hash autoral atribuído a upstream.
Instalar e selecionar continuam gestos do dono, sem atualização automática ou
sobrescrita de catálogo instalado.

Precisão da coordenação antes do freeze D353: o auxiliar original excluído foi
substituído por guia curto autoral `references/donna-transitions.md`. Não se
consultou o auxiliar incerto para redigir regras, estrutura ou exemplos. O
sourceDigest upstream não cobre o guia autoral; somente o manifesto de assets
e o envelope adaptado o cobrem. A autoria própria sob MIT segue o regime desta
adaptação, sem alterar licenças do produto ou relicenciar os titulares externos.

## Codync — lote 2, item 1: uso Codex atual (06/11/2026)

Fontes consultadas pela API GitHub, com autoria e licença nos próprios pins:

| Origem (SHA completo) | Autor do commit | Destino e adaptação em 06/11/2026 |
| --- | --- | --- |
| [host/src/usage/codex.rs e orientação em host/src/usage/mod.rs](https://github.com/leepokai/Codync/commit/ded969dc97b7a0df78e37dc5f011c60dfee90887), `ded969dc97b7a0df78e37dc5f011c60dfee90887` | Sean Ferguson | `host/src/fork/codex_usage/mod.rs`: consulta initialize → account/rateLimits/read, bucket Codex, expiração e limpeza de falha; `host/src/usage.rs`: gancho e orientação. Transporte Acp/HOME do upstream não copiado: reutiliza o lifecycle Donna em `host/src/fork/engine_setup/process.rs` com ambiente global filtrado. |
| [host/src/usage/mod.rs, apenas substituição de leitura completa](https://github.com/leepokai/Codync/commit/e9980b3bcdf01c53ebbfff931b05eb03afc9addc), `e9980b3bcdf01c53ebbfff931b05eb03afc9addc` | forwardxu <forwardxu@apache.org> | `host/src/fork/codex_usage/mod.rs`, função replace: remove janelas omitidas e provider vazio, apenas Codex. Nenhum outro provedor/tela/analytics portado. |

Ambos sob Apache-2.0, conferida em LICENSE de cada SHA. NOTICE de ded969dc
identifica Copyright 2026 Po Kai Lee e os contribuidores Codync; LICENSE e NOTICE
do repositório foram preservados integralmente. Testes Python/Rust deste recorte
são próprios da Donna, não cópia de fixtures upstream. A adaptação mantém faixas
Donna 0–100 e não registra respostas brutas, tokens, identidades ou caminhos.

## Codync — lote 2, item 2: desbloqueio do chaveiro (06/11/2026)

Fonte primária consultada pela API GitHub: commit
[`25202822c17e12d0581cd043e331b11afea3ff8e`](https://github.com/leepokai/Codync/commit/25202822c17e12d0581cd043e331b11afea3ff8e),
pai `47ebd594cd52f5bf6b12d7c323eb5112856c0b26`, autor leepokai, de 29/10/2026, Refs #260, sob Apache-2.0.
A API registra a coautoria histórica `Claude Opus 5.5 <noreply@anthropic.com>`;
esse crédito não representa revisão ou aceite desta rodada. LICENSE e NOTICE
locais permanecem byteiguais.

| Origem no commit | Destino e adaptação em 06/11/2026 |
| --- | --- |
| `host/src/market/vault.rs` | `host/src/market/vault_unlock.rs` e gancho curto em `host/src/market/vault.rs`: trava assíncrona, guarda movida para worker, timeout e pré-desbloqueio da coleção padrão. Preservados serviço próprio por Environment, cifra, legado e política de cofre único. A exceção genérica upstream virou somente `Error::NoResult` (alias padrão ausente), propagando demais erros. |
| `host/src/store/mod.rs`, `host/src/store/schema.rs` | Campo/inicialização `vault_unlock`, sem migração de banco. |
| `host/src/main.rs` | `exit(0)` só após o encerramento completo de serve. |
| `host/Cargo.toml`, `host/Cargo.lock` | Dependência Linux direta `dbus-secret-service` 4.1.0 vendored, já resolvida; lock recebe somente a ligação no pacote `codync-host`. |
| `docs/features/connector-credentials.md` | Seção de desbloqueio adaptada para PT-BR com prazo, disponibilidade do Hub e limites aceitos; manual embutido acompanha o comportamento. |

`web/content/docs/linux-servers.mdx` não foi portado. Testes sidecar `cfg(test)`
são próprios da Mexerica. A API de `keyring` 3.6.3 (MockCredential/CredentialApi)
e o código-fonte de `dbus-secret-service` 4.1.0 foram consultados via arquivos
oficiais de crates.io, sem copiar suas implementações ou executar chaveiro.

## Opensource UI — papéis de parede em código (DON-909)

Referências visuais e fórmulas de gradiente: [Opensource UI](https://opensourceui.in),
por Bidyut Kundu. Licença MIT verificada em 06/11/2026 no
[repositório de origem](https://github.com/bidyut10/opensourceui/blob/main/LICENSE).
Os sete componentes consultados foram Dark Aurora, Dark Carbon Spotlight, Dark Ember,
Dark Midnight Mesh, Dark Rose Noir, Frost Mesh e Sunrise Horizon (URLs e evidência
no relatório DON-909 em `(arquivo de design privado do projeto de origem, fora deste repositório)`). Adaptação para o sistema de
aparência da Mexerica, com versões claras/escuras e a composição Petróleo da Mexerica, conforme a seleção D381 do dono.
Nenhum pacote de terceiros foi acrescentado. Aviso preservado por D75:

```text
MIT License

Copyright (c) 2026 Bidyut Kundu

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Guia do Google e escopos do grupo Google — Tucano (Marketplace, telas v3)

Adaptação em 16/11/2026 de [Tucano](https://github.com/eduwxyz/tucano), commit
`90d144a4270509247fef05ed88c6244b15223007`,
[Apache-2.0](https://github.com/eduwxyz/tucano/blob/90d144a4270509247fef05ed88c6244b15223007/LICENSE)
(sem arquivo NOTICE nesse commit). Origem: `src/core/guide/google.md` e
`src/shared/connector-catalog.ts` (L79–103, L286–287, L317–394). Destino: textos das telas
`docs/propostas/marketplace-20261116/mock-mkt3-*` (`gerar3.mjs`, HTML e PNGs derivados) e a pesquisa
`docs/propostas/marketplace-20261116/pesquisa-tucano-google.md`. O que adaptamos: a estrutura e a ordem
dos 6 passos do guia "app seu no Google Cloud" e a lista de serviços e escopos, em texto nosso, reescrito
e menor. Nenhum trecho foi copiado literalmente e nenhum código foi copiado.

## Terminal de login — Tucano (MEX-1289)

Adaptação em 15/11/2026 de [Tucano](https://github.com/eduwxyz/tucano), commit
`9474545f0415f4bb2a439e1a7727912254b69972`,
[Apache-2.0](https://github.com/eduwxyz/tucano/blob/9474545f0415f4bb2a439e1a7727912254b69972/LICENSE).
Trechos: tabela `loginCommand` e dicas em `src/core/login-terminal.ts` (argv vazio
do agy, `auth login` do OpenCode, `login --device-auth` do Grok, `login` com
`NO_OPEN_BROWSER=1` do Cursor e `auth` do Cline); reconhecimento das linhas
tabuladas de `agy models` em `src/core/engines/{antigravity,detect}.ts`.
Destinos: `host/src/fork/terminal_login.rs`, `host/src/agent/term.rs`, testes
correspondentes e dicas em `apps/desktop/src/renderer/lib/fork-i18n/*/extensoes/geral.ts`.
A tabela foi traduzida para argumentos diretos em Rust e as dicas adaptadas à
voz da Mexerica. A separação por Space/perfil, cofre e guardas ACS4 são próprias.
`vt-screen.ts` foi considerado; seu emulador não foi copiado porque o painel já
usa xterm no `SetupTerminalView`. Não copiamos o terminal livre do Tucano, sua
persistência, nem a implementação de turnos do agy. LICENSE e NOTICE preservados.

Os testes `host/tests/motores_e2e.rs` e `host/tests/fixtures/motor_cli.py` vêm da
frente F3 da própria Mexerica, PR #836, head
`95086f59b4a4cb01015cc2915e07a3f2182e30cc`, atualizados na main. Os roteiros de
terminal desse arquivo permanecem ignorados; as provas de PTY com autoridade ACS4,
escapes e supressão de eco estão em `terminal_login/lifecycle_tests.rs`. Os demais
`ignore` da frente F3 mantêm seus motivos e não são declarados comprovados aqui.
### pi-mono — motor vendorizado (MEX-1293)

```text
MIT License

Copyright (c) 2025 Mario Zechner

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

A cópia íntegra permanece em `motor/vendor/pi-mono/LICENSE` e a origem exata em
`motor/vendor/pi-mono/UPSTREAM.md`.
