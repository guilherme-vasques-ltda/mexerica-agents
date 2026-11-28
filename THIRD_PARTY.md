# Mexerica — procedência e atribuições

O histórico, `LICENSE` e `NOTICE` preservam a origem do fork Codync (Apache-2.0).
A identificação Mexerica (antes DONNA Bot) não substitui licenças de componentes, titulares,
avisos por arquivo ou condições de marca/serviços. A licença Apache-2.0 da base
não se estende por inferência a todo código, fonte, skill ou ativo do conjunto.

Este arquivo registra só o material de terceiros que está de fato nesta cópia: código ou arquivos
copiados (mesmo com ajustes), ativos (fontes, ícones, logos, skills inteiras) e dependências.
Código ou ativo de terceiros copiado ganha entrada, qualquer que seja o tamanho. Não ganham entrada o que foi
reescrito, traduzido ou portado, as ideias, a estrutura, as listas e o texto corrido (frase ou parágrafo)
aproveitado de outro lugar.

## Escopo desta cópia

Conferido em 22/11/2026 contra a árvore deste repositório e, para cada trecho copiado, contra a origem no commit citado.
Não é parecer jurídico.

- Código derivado do Codync v2.20.3 (modificações em `MODIFICACOES.md`) e trechos de commits posteriores do Codync (tabela abaixo).
- Padrões de redação do ai-memory (MIT) em `host/crates/memory/src/redact.rs` e receitas de fundos do Opensource UI (MIT) em `apps/desktop/src/renderer/lib/fork-wallpapers-code.ts`.
- Código do OpenMausBot (Apache-2.0) em `apps/desktop/src/renderer/lib/routine-calendar.ts`.
- Fixture de teste com o esquema do wacli (MIT) em `host/src/fork/whatsapp/wacli_fixture.sql`.
- `LICENSE` de Exa e de Supabase (MIT), empacotada pelo Hub com as skills de conector em `host/catalogo/skills/`.
- Skills inteiras de terceiros no catálogo do Hub (`host/src/fork/skill_catalog/registry.json`), com a licença de cada uma.
- pi-mono (MIT) vendorizado em `motor/vendor/pi-mono/`, com as dependências npm do motor (Anthropic, OpenAI, Google).
- Fontes da interface (SIL OFL 1.1, texto ao lado de cada arquivo) em `apps/desktop/src/renderer/assets/fonts/`.
- Pacotes npm de produção do desktop listados em `packaging/avisos/INVENTARIO-DEPENDENCIAS.md`, incluindo `lucide` (ISC), `@noble/ciphers` (MIT), `@noble/curves` (MIT), `@noble/hashes` (MIT), `@clerk/clerk-js` (MIT) e `electron-updater` (MIT).
- Helper de tela do Linux (`apps/screen-linux/`, pacote `codync-screen`): dependências diretas declaradas em `apps/screen-linux/Cargo.toml` e fixadas em `apps/screen-linux/Cargo.lock` listadas na seção "Helper de tela do Linux (apps/screen-linux)" abaixo; o grafo transitivo completo de `apps/screen-linux/Cargo.lock` ainda não foi consolidado em `packaging/avisos/INVENTARIO-DEPENDENCIAS.md`.
- As logos dos serviços do catálogo de conectores em `apps/desktop/src/renderer/assets/marcas/` (marcas de seus titulares, só para identificar o serviço).
- Electron (MIT), WebRTC de `stasel/WebRTC` (BSD-3-Clause) e o driver Cua (MIT): os textos de licença estão em `packaging/avisos/` e `packaging/cua-driver/`.
- Clientes móveis em `apps/mobile/` (seção "Clientes móveis (apps/mobile)" abaixo): xterm.js, ThinkingOrbs, fontes Inter, Nunito e DM Sans (SIL OFL 1.1) e vetores do Material Symbols incluídos no repositório, e as dependências declaradas nos manifestos iOS e Android.
- Código de Conduta em `CODE_OF_CONDUCT.md`: tradução pt-BR do Contributor Covenant 2.1 (CC BY 4.0), com o contato preenchido.
- Registro de tstack (MIT), mantido: os arquivos de destino citados (`.claude/`, `AGENTS.md`, `.github/`) não estão nesta cópia.

Registre abaixo cada cópia de código ou de ativo de terceiros, inclusive com ajustes. Adaptações reescritas e traduções não entram.

## Origem do material copiado

| Origem (repositório/URL) | Commit | Arquivo de origem | Arquivo de destino | Licença | Data (AAAA-MM-DD) |
|---|---|---|---|---|---|
| [Contributor Covenant](https://www.contributor-covenant.org/pt-br/version/2/1/code_of_conduct/) — Ethical Source | versão 2.1 (tradução pt-BR) | `code_of_conduct.md` | `CODE_OF_CONDUCT.md` (contato preenchido) | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) | 2026-11-27 |
| [Exa MCP Server](https://github.com/exa-labs/exa-mcp-server) — Exa Labs | `a02cd621534313b8f01dbb03b829dac448decb8b` | `LICENSE` | `host/catalogo/skills/exa/exa-pesquisar-na-web/LICENSE` | [MIT](https://github.com/exa-labs/exa-mcp-server/blob/a02cd621534313b8f01dbb03b829dac448decb8b/LICENSE) | 2026-11-18 |
| [Supabase Agent Skills](https://github.com/supabase/agent-skills) — Supabase | `c9be0e931b7930f7d02126d04774d904c381e7d7` | `LICENSE` | `host/catalogo/skills/supabase/supabase-revisar-seguranca/LICENSE` | [MIT](https://github.com/supabase/agent-skills/blob/c9be0e931b7930f7d02126d04774d904c381e7d7/LICENSE) | 2026-11-18 |
| [Codync](https://github.com/leepokai/Codync) | `219935410dffccb950f90d0565bd1d9f1661bd5b` | `apps/desktop/src/renderer/lib/modal-stack.ts` (`stackModals`) e `modal-stack.test.ts` | `apps/desktop/src/renderer/lib/fork-modal-stack.ts` e `fork-modal-stack.test.ts` (modificados) | [Apache-2.0](https://github.com/leepokai/Codync/blob/219935410dffccb950f90d0565bd1d9f1661bd5b/LICENSE) | 2026-11-06 |
| [Codync](https://github.com/leepokai/Codync) | `57537a1e5d52f27c595af7415cc4f69a79dff080`, `bb242a7df4a9479fac622657a70f4007cb690fe3` | `host/src/service/addresses.rs` | `host/src/fork/pairing_addresses.rs` (modificado) | [Apache-2.0](https://github.com/leepokai/Codync/blob/57537a1e5d52f27c595af7415cc4f69a79dff080/LICENSE), [LICENSE do segundo commit](https://github.com/leepokai/Codync/blob/bb242a7df4a9479fac622657a70f4007cb690fe3/LICENSE) | 2026-11-06 |
| [Codync](https://github.com/leepokai/Codync) | `25202822c17e12d0581cd043e331b11afea3ff8e` | `host/src/market/vault.rs` (desbloqueio do chaveiro) | `host/src/market/vault_unlock.rs` (modificado) | [Apache-2.0](https://github.com/leepokai/Codync/blob/25202822c17e12d0581cd043e331b11afea3ff8e/LICENSE) | 2026-11-06 |
| [ai-memory](https://github.com/akitaonrails/ai-memory) — Fabio Akita | `a295983828901aa612d37dabd6045500346d457a` | `crates/ai-memory-core/src/sanitize.rs` (padrões de redação) | `host/crates/memory/src/redact.rs` (modificado) | MIT (aviso abaixo) | 2026-10-20 |
| [Opensource UI](https://github.com/bidyut10/opensourceui) — Bidyut Kundu | `892f486fdf0d46a44b21ddf3fa32a41a3a71173a` | `components/background-gradient/*` | `apps/desktop/src/renderer/lib/fork-wallpapers-code.ts` (receitas dos fundos) | MIT (aviso abaixo) | 2026-11-06 |
| [OpenMausBot](https://github.com/milind-soni/OpenMausBot) | `615fe65e17f88d99f12d34682750d9ae2fa402bc` | `src/lib/routine-calendar.ts` (datas, encaixe, colunas de sobreposição, rótulo GMT) | `apps/desktop/src/renderer/lib/routine-calendar.ts` (modificado) | Apache-2.0 (aviso abaixo) | 2026-10-21 |
| [tstack](https://github.com/t3dotgg/tstack) — Theo Browne | `1085050f3baed1ac724caa4a8166145fb2ac88af` | `skills/babysit-pr/scripts/pr-wait` | `.claude/skills/acompanhar-pr/scripts/pr-wait` (cópia sem alteração) | MIT (aviso abaixo) | 2026-11-04 |
| [tstack](https://github.com/t3dotgg/tstack) — Theo Browne | `1085050f3baed1ac724caa4a8166145fb2ac88af` | `skills/sol-loop`, `babysit-pr`, `file-pr`, `full-send`, `full-stop` e `leaving-pr-comments` (`SKILL.md`, texto adaptado) | `AGENTS.md` (Rodadas de revisão, escopo e PR), `.claude/skills/{abrir-pr,acompanhar-pr,full-stop}/SKILL.md`, `.github/pull_request_template.md` (reescritos em PT-BR para as regras da Mexerica) | MIT (aviso abaixo) | 2026-11-04 |
| [wacli](https://github.com/openclaw/wacli) — Peter Steinberger (v0.25.1) | `b7603b697e69be591afa83379ced1e73177604c6` | `internal/store/schema.sql` (tabelas `chats`, `contacts`, `messages`, `call_events` e índices `idx_call_events_*`) | `host/src/fork/whatsapp/wacli_fixture.sql` (cópia sem alteração; só fixture de teste do R3/R5c do WhatsApp) | MIT (aviso abaixo) | 2026-11-05 |
| [pi-mono](https://github.com/badlogic/pi-mono) — Mario Zechner | `22fc026b872b4a7df22ffa94e8fa00b873128054` | `packages/schema/src`, `packages/telemetry/src`, `packages/agent/src`, subconjunto de `packages/ai/src` (sem catálogos gerados nem `compat`), `packages/coding-agent/src/modes/rpc/jsonl.ts` | `motor/vendor/pi-mono/packages/{schema,telemetry,ai,agent,coding-agent}/` (cópia com ganchos curtos listados em `motor/vendor/pi-mono/PATCHES.md`; `LICENSE` MIT preservado na pasta raiz da cópia) | MIT (aviso abaixo) | 2026-11-16 |
| [@anthropic-ai/sdk](https://www.npmjs.com/package/@anthropic-ai/sdk) — Anthropic | `0.129.0` (versão fixa no `motor/package-lock.json`) | SDK oficial da Anthropic, dependência do pi-ai vendorizado | `motor/node_modules` (instalação; declaração fixa em `motor/vendor/pi-mono/packages/ai/package.json` e `motor/package-lock.json`) | MIT | 2026-11-16 |
| [openai](https://www.npmjs.com/package/openai) — OpenAI | `7.19.0` (versão fixa no `motor/package-lock.json`) | SDK oficial da OpenAI, dependência do pi-ai vendorizado | `motor/node_modules` (instalação; declaração fixa em `motor/vendor/pi-mono/packages/ai/package.json` e `motor/package-lock.json`) | Apache-2.0 | 2026-11-16 |
| [@google/genai](https://www.npmjs.com/package/@google/genai) — Google | `2.32.0` (versão fixa no `motor/package-lock.json`) | SDK oficial do Google, dependência do pi-ai vendorizado | `motor/node_modules` (instalação; declaração fixa em `motor/vendor/pi-mono/packages/ai/package.json` e `motor/package-lock.json`) | Apache-2.0 | 2026-11-16 |

Antes de distribuir ou entrar em uso de empresa, revise as licenças registradas.

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

### Opensource UI

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

### OpenMausBot

Apache License 2.0 (<https://www.apache.org/licenses/LICENSE-2.0>), o mesmo texto
de `LICENSE` deste repositório. O arquivo copiado
(`apps/desktop/src/renderer/lib/routine-calendar.ts`) leva no topo a origem e a nota de alteração.
Linha do `NOTICE` do OpenMausBot:

```text
OpenMausBot
Copyright 2026 Milind Soni and OpenMausBot contributors
```

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

Pacote npm `lucide`, dependência do desktop (os ícones vão no código do renderer).

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

### pi-mono — motor vendorizado

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

### @noble/ciphers (ChaCha20-Poly1305)

Dependência de produção `@noble/ciphers` **2.4.0**, sem adaptações, importada de
`@noble/ciphers/chacha.js` por `apps/desktop/src/main/fork-cloud/crypto.ts`.
Origem: <https://github.com/paulmillr/noble-ciphers/tree/2.4.0>; tarball e
integridade em `apps/desktop/package-lock.json`. MIT conferida no pacote e idêntica à tag; SHA256 do LICENSE:
`f36671a5487c9c5050efacb58011c37c24c55a889803cb036cf9d9a6347c1e2d`. O pacote de produção conserva seu LICENSE.

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

## Skills de terceiros no catálogo do Hub

`host/src/fork/skill_catalog/registry.json` (compilado no Hub) embute o texto, as referências e o arquivo de licença
de cada skill abaixo; `source-manifest.json` registra, por arquivo, o commit, o blob Git e o SHA-256 da origem.
Os arquivos são os da origem, sem alteração, exceto os dois `SKILL.md` indicados. Cada pacote leva a licença da origem.
As demais skills listadas no manifesto não têm o texto no registro e não entram aqui.

| Skill | Origem | Commit | Arquivos da origem | Licença | Data (AAAA-MM-DD) |
|---|---|---|---|---|---|
| `security-audit` | [cloudflare/security-audit-skill](https://github.com/cloudflare/security-audit-skill) — Cloudflare, Inc. | `c1c8a8c1471069fb0e188eeaff69b8e8db6564a8` | 17 arquivos | MIT (aviso na seção `security-audit-skill`) | 2026-11-07 |
| `impeccable` | [pbakaus/impeccable](https://github.com/pbakaus/impeccable) | `936867543ac2576920d663cc9803597f6044f3ac` | 3 arquivos | Apache-2.0 (`LICENSE` e `NOTICE.md` no pacote) | 2026-10-25 |
| `frontend-design` | [anthropics/skills](https://github.com/anthropics/skills) | `dbd4588f9e1033efb41dad4bef2f7947c8993d44` | 2 arquivos | Apache-2.0 (`LICENSE.txt` no pacote) | 2026-10-25 |
| `find-skills` | [vercel-labs/skills](https://github.com/vercel-labs/skills) — Vercel, Inc. | `a80e4c9d12d607631249d4d7d059f835d4b2f13a` | 2 arquivos | MIT (`LICENSE` no pacote) | 2026-10-25 |
| `grilling`, `research` | [mattpocock/skills](https://github.com/mattpocock/skills) — Matt Pocock | `5beb35b040e3de97e54fa9e386d7e42d15c295c2` | 2 arquivos por skill | MIT (`LICENSE` no pacote) | 2026-10-25 |
| `brainstorming` | [obra/superpowers](https://github.com/obra/superpowers) — Jesse Vincent | `2ce8aa15d05bd57213e6f746f953aafb7ded7a8b` | 2 arquivos | MIT (`LICENSE` no pacote) | 2026-10-25 |
| `copywriting` | [coreyhaines31/marketingskills](https://github.com/coreyhaines31/marketingskills) — Corey Haines | `e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b` | 5 arquivos | MIT (`LICENSE` no pacote); `copywriting` traz também a `LICENSE` da Profound ([skills-marketing-library](https://github.com/retieedra-profound/skills-marketing-library), commit `80293f156c27d3f90aedebfb3c27309769b2eec6`, MIT). O `SKILL.md` de `copywriting` tem a adaptação descrita em `ADAPTATION.md` | 2026-11-06 |
| `content-strategy`, `marketing-psychology` | [coreyhaines31/marketingskills](https://github.com/coreyhaines31/marketingskills) — Corey Haines | `e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b` | 5 e 3 arquivos | MIT (`LICENSE` no pacote) | 2026-10-25 |
| `hyperframes` | [heygen-com/hyperframes](https://github.com/heygen-com/hyperframes) | `9aadc0b9b99d92afd25215228878cee23ea47a0f` | 25 arquivos | Apache-2.0 (`LICENSE` e `CREDITS.md` no pacote) | 2026-10-25 |
| `boat` | [davidondrej/skills](https://github.com/davidondrej/skills) — David Ondrej | `8605863f211961c96208fe625d4024609cd7dbd5` | 3 arquivos | MIT (`LICENSE` no pacote) | 2026-10-25 |
| `stop-slop` | [hardikpandya/stop-slop](https://github.com/hardikpandya/stop-slop) — Hardik Pandya | `8da1f030185bdfe8471220585162991eaeb970e9` | 5 arquivos | MIT (`LICENSE` no pacote) | 2026-11-06 |
| `copy-review-panel` | [Yuzzyuk/marketing-os](https://github.com/Yuzzyuk/marketing-os) — Marketing OS contributors | `bb67dff5f04b390e861ee11433166e4519e7f4c0` | 2 arquivos | MIT (`LICENSE` no pacote). O `SKILL.md` adapta `skills/marketing-os/references/copy.md`, com a tabela idêntica à da origem | 2026-11-06 |

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

## Clientes móveis (apps/mobile)

Os clientes iOS e Android desta cópia vivem em `apps/mobile/`. Esta seção lista o que está copiado dentro do repositório e as
dependências declaradas nos manifestos (`apps/mobile/project.yml`, `apps/mobile/ios/Kit/Package.swift`,
`apps/mobile/android/gradle/libs.versions.toml`, `apps/mobile/android/app/build.gradle.kts` e `apps/mobile/android/core/build.gradle.kts`).
As versões abaixo são as fixadas nesses arquivos em 21/11/2026.

### Material incluído no repositório

| Material | Onde fica | Licença | Aviso de licença |
| --- | --- | --- | --- |
| xterm.js: `@xterm/xterm` 6.0.0, `@xterm/addon-fit` 0.11.0 e `@xterm/addon-web-links` 0.12.0 (arquivos minificados com esbuild 0.28.2) | `apps/mobile/android/app/src/main/assets/terminal/` | MIT | `LICENSE.xterm`, `LICENSE.addon-fit` e `LICENSE.addon-web-links` na mesma pasta; origem, versão e hashes em `vendor-manifest.json` |
| ThinkingOrbs — Copyright (c) 2026 Jakub Antalik | `apps/mobile/ios/Kit/Sources/CodyncKit/Design/ThinkingOrb*.swift` e `apps/mobile/android/design/src/main/kotlin/com/codync/android/design/ThinkingOrb*.kt` | MIT | `apps/mobile/ios/Kit/Sources/CodyncKit/Resources/ThirdPartyNotices/thinking-orbs-LICENSE.txt` (iOS) e `apps/mobile/android/app/src/main/assets/licenses/thinking-orbs-LICENSE.txt` (Android) |
| Inter (Copyright 2016 The Inter Project Authors) | `apps/mobile/android/design/src/main/res/font/inter.ttf` | SIL OFL 1.1 | `apps/mobile/android/app/src/main/assets/licenses/inter-OFL.txt` |
| Nunito (Copyright 2014 The Nunito Project Authors) | `apps/mobile/ios/Kit/Sources/CodyncUI/Resources/Redesign/Fontes/Nunito-VariableFont_wght.ttf` | SIL OFL 1.1 | `Nunito-OFL.txt` na mesma pasta |
| DM Sans (Copyright 2014 The DM Sans Project Authors) | `apps/mobile/ios/Kit/Sources/CodyncUI/Resources/Redesign/Fontes/DMSans-VariableFont_opsz-wght.ttf` | SIL OFL 1.1 | `DMSans-OFL.txt` na mesma pasta |
| Material Symbols Rounded — Google (vetores `sf_*.xml`, gerados por `apps/mobile/android/tools/symbols.py`) | `apps/mobile/android/design/src/main/res/drawable/` | Apache-2.0 | texto da Apache-2.0 em `LICENSE` na raiz deste repositório |

Notas:

- As fontes `CodyncBot-*.ttf` e `CodyncOrb-*.ttf` (em `apps/mobile/ios/Widgets/Resources/Fonts/`) vêm do Codync v2.20.3 e fazem parte do código derivado (Apache-2.0, ver `MODIFICACOES.md`).
- Os vetores `apple_logo.xml` (Simple Icons, CC0) e `google_logo.xml`, além de 32 vetores `agent_*.xml` (gerados por `apps/mobile/android/tools/agent_icons.py`) em `apps/mobile/android/app/src/main/res/drawable/`, e os conjuntos `apps/mobile/ios/Kit/Sources/CodyncUI/Resources/AgentIcons.xcassets/` e `apps/mobile/ios/Kit/Sources/CodyncKit/Resources/ProviderIcons.xcassets/` no iOS identificam serviços, provedores ou agentes de código e pertencem aos seus titulares (uso nominativo herdado do Codync v2.20.3; alinhamento com `TRADEMARK.md` e `MODIFICACOES.md` pendente de revisão).
- Os arquivos `apps/mobile/shared/Config/*.plist` ficam neutros (sem nuvem e sem conta); os valores locais ficam em `*.local.plist`, ignorado pelo Git.

### Dependências declaradas nos manifestos

Não estão copiadas no repositório: o gerenciador de pacotes (Swift Package Manager ou Gradle) as baixa na construção.

| Dependência | Versão | Plataforma | Licença | Observação |
| --- | --- | --- | --- | --- |
| [Clerk iOS SDK](https://github.com/clerk/clerk-ios) | 1.5.6 | iOS | MIT | `apps/mobile/project.yml` |
| Clerk Android (`com.clerk:clerk-android-api`) | 1.1.11 | Android | MIT | |
| [WebRTC](https://github.com/stasel/WebRTC) (`stasel/WebRTC`) | 153.0.0 | iOS | BSD-3-Clause | aviso em `packaging/avisos/webrtc-stasel-LICENSE.md` |
| WebRTC Android (`io.github.webrtc-sdk:android`) | 125.6422.07 | Android | BSD-3-Clause | |
| [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) | 1.18.0 | iOS | MIT | |
| Firebase BoM e Firebase Messaging; plugin `google-services` 4.5.0 | 34.19.0 | Android | Apache-2.0 | o iOS usa APNs, sem Firebase |
| Vosk Android (`com.alphacephei:vosk-android`) | 0.3.75 | Android | Apache-2.0 | `vosk-APACHE-2.0.txt` em `apps/mobile/android/app/src/main/assets/licenses/` |
| Modelo de voz `vosk-model-small-en-us` | 0.15 | Android | Apache-2.0 | baixado na construção, com SHA-256 fixado e conferido em `apps/mobile/android/app/build.gradle.kts` |
| Bouncy Castle (`bcprov-jdk18on`) | 1.86 | Android | Bouncy Castle License (estilo MIT) | |
| JNA (`net.java.dev.jna:jna`) | 5.18.1 | Android | Apache-2.0 ou LGPL-2.1+ (usada sob Apache-2.0) | `jna-APACHE-2.0.txt` e `jna-NOTICE.txt` em `apps/mobile/android/app/src/main/assets/licenses/` |
| CommonMark (`commonmark`, `commonmark-ext-gfm-tables`, `commonmark-ext-gfm-strikethrough`) | 0.30.0 | Android | BSD-2-Clause | |
| Coil (`coil-compose`, `coil-network-okhttp`) | 3.4.0 | Android | Apache-2.0 | |
| ZXing Core (`com.google.zxing:core`) | 3.5.4 | Android | Apache-2.0 | |
| Kyant AndroidLiquidGlass (`backdrop`, `shapes`) | 2.0.1 e 1.2.1 | Android | Apache-2.0 | |
| JUnit | 4.13.2 | Android | EPL-1.0 | só testes unitários; não vai no APK |
| `com.android.tools:desugar_jdk_libs` | 2.1.5 | Android | GPL-2.0 with Classpath Exception, dependência de build/runtime, não vendorizada | usado na construção (desugaring de bibliotecas Java) |
| esbuild | 0.28.2 | desenvolvimento | MIT | só para regenerar os arquivos do terminal (`apps/mobile/android/tools/vendor-terminal.py`); não vai no APK |

## Arte própria da Mexerica (restaurada em 19/11/2026)

A cópia inicial trocou a arte própria da Mexerica por imagens neutras geradas por código. Por decisão do dono do projeto em 19/11/2026
("sim, libera tudo, as artes são nossas, são minhas..." e "sim, volte. tudo."), os arquivos originais voltaram do repositório de origem
(revisão `d6a75dd0afb0bcddc917bcebcf7c8701085fc0f9`): 1.037 deles com exatamente os mesmos bytes, 34 PNGs recomprimidos sem perda (ver "Credenciais de
conteúdo" abaixo) e 4 textos descritivos com uma nota e os novos hashes. `brand/ARTE-HASHES.tsv` lista cada arquivo (grupo, regra de uso, SHA-256 e tamanho
da origem, SHA-256 e tamanho do arquivo atual, e o identificador do blob Git de origem). Os 1.075 arquivos foram conferidos e não trazem metadados embutidos que identifiquem
pessoa ou máquina (nenhum campo EXIF/XMP, nenhum texto de PNG ou de WebP); os PDF de logo têm só `Creator: Chromium`, `Producer: Skia/PDF` e a data de exportação (07/11/2026).

**Credenciais de conteúdo nos próprios arquivos.** Os originais de 34 PNGs traziam credenciais C2PA embutidas (bloco `caBX`), assinadas por "OpenAI Media Service /
OpenAI OpCo, LLC" e datadas de 07/11/2026. Elas registravam o agente de software "ChatGPT", versão "gpt-image", e a origem digital "mídia gerada por
algoritmo treinado" (IPTC `trainedAlgorithmicMedia`). São os 20 fundos de chat (`brand/assets/fundos-chat/`), os 10 personagens por papel
(`brand/assets/personagens/`) e as 4 fontes originais do ícone (`brand/assets/icon/00_fontes_originais/`). A coluna `c2pa_no_original` de
`brand/ARTE-HASHES.tsv` marca cada um. Os demais arquivos não traziam essa marca, o que não prova como foram feitos. O dono do projeto não informou a ferramenta
de geração; este registro só repete o que as credenciais afirmavam. Ele não conferiu os termos de uso da ferramenta sobre titularidade ou uso comercial das imagens
geradas nem a cadeia de direitos sobre os insumos. Nada aqui é parecer jurídico.

**Recompressão desses 34 PNGs (decisão do dono, 19/11/2026).** Para reduzir o peso do repositório, o dono decidiu em 19/11/2026 recomprimir esses 34 PNGs
sem perda. A recompressão (oxipng, nível 4, sem redução de tipo de cor nem alteração de canal alfa) removeu o bloco `caBX` com as credenciais C2PA de cada
arquivo; **os arquivos atuais não carregam mais essa marca de origem**, e esta seção a substitui por escrito. Os pixels decodificados são idênticos aos dos
originais: conferido nos 34 por dois decodificadores independentes (PIL e ffmpeg, em RGBA de 64 bits), com o mesmo cabeçalho de imagem (largura, altura,
profundidade e tipo de cor) e nenhum outro bloco removido. Os originais intactos, com as credenciais, continuam no repositório de origem, na revisão
`d6a75dd0afb0bcddc917bcebcf7c8701085fc0f9`. `brand/ARTE-HASHES.tsv` traz, para cada um, o SHA-256 da origem e o do arquivo atual.

| Grupo | Onde fica | Arquivos | Licença declarada na origem |
| --- | --- | ---: | --- |
| Mexeriquinhas (pelúcia 3D, imagens WebP e atlas) | `apps/desktop/src/renderer/lib/mexeriquinhas/assets/` | 595 | Apache-2.0 (README da biblioteca: "código, prompts e ilustrações originais para Apache 2.0") |
| Ícones opcionais do app (Ajustes › Ícone do app) | `apps/desktop/resources/app-icons/` | 20 | Apache-2.0 para as 19 opções das rodadas 2 e 3, com o texto em `LICENSE` da pasta; `padrao.png` é o ícone oficial reduzido e não declara licença própria |
| Logo, mascote, símbolo, ícone do app, favicons e ícone da barra de menus | `brand/assets/logo/`, `apps/desktop/resources/{icon*,brand,tray,AppIcon.icon}`, `apps/desktop/src/renderer/assets/brand/` | 136 | nenhuma declarada na origem: marca reservada (ver `TRADEMARK.md`) |
| Variantes e fontes do ícone do app | `brand/assets/icon/` | 185 | nenhuma declarada na origem |
| Fundos de chat e papéis de parede | `brand/assets/fundos-chat/`, `apps/desktop/src/renderer/assets/wallpapers/` | 128 | nenhuma declarada na origem |
| Personagens por papel (CEO, Assistente, Financeiro etc.) | `brand/assets/personagens/` | 11 | nenhuma declarada na origem ("originais do dono, 07/11/2026") |

**Regra de uso (decisão do dono, 19/11/2026).** Dois grupos seguem Apache-2.0, como a origem declara: as 595 Mexeriquinhas e as 19 opções de
ícone das rodadas 2 e 3. Todos os outros grupos da tabela (logo, mascote, símbolo, ícone do app, favicons e bandeja; variantes e fontes do ícone;
fundos de chat e papéis de parede; personagens; e o `padrao.png`, que é o ícone oficial reduzido) **não** são licenciados sob Apache-2.0:
a Mexerica reserva os direitos que tiver sobre essas imagens e não concede licença de reuso fora dela (este texto não afirma quem é o titular de direito autoral de cada uma), e o uso permitido é dentro da Mexerica como distribuída, sem reuso em outros produtos. Esta cópia não
escolhe uma licença onde a origem não declara. A lista de caminhos de cada regra, com a decisão do dono citada, está em
[brand/LICENCA-DA-ARTE.md](brand/LICENCA-DA-ARTE.md). Nenhum advogado leu este texto e ele não é parecer jurídico.
Dentro das pastas acima ficam também textos que descrevem os arquivos (`LEIA-ME`, tabelas de medidas e PDF de logo).

**Continuam neutros nesta cópia:** os 43 ícones de agentes de terceiros (`apps/desktop/src/renderer/assets/agents/`), o logotipo da Apple no login,
`assets/google.svg` e `assets/computer-linux.svg`. A liberação do dono cobre só a arte da Mexerica, não marcas de terceiros. As logos dos
conectores seguem na seção seguinte.

## Marcas de serviços do catálogo de conectores (2026-11-17)

Cada serviço do catálogo de conectores (Marketplace) aparece com a logo do próprio fornecedor, em vez do globo genérico.
Os arquivos ficam no app, em `apps/desktop/src/renderer/assets/marcas/`, e são mapeados por `views/marketplace/marcas.ts`
(id da ficha do Hub → arquivo). **Nada é baixado em execução**: cada arquivo foi baixado uma vez, em 17/11/2026, da fonte
oficial do próprio fornecedor listada abaixo, sem alteração (nem recorte nem recolorização). Sem arquivo para o serviço, o
app mostra o globo.

**Uso e titularidade.** Todas são marcas e logos de seus titulares (os fornecedores da tabela abaixo, como Linear, Composio, Slack/Salesforce, Google,
Microsoft, GitHub e Meta/Instagram), que **não** são cobertas pela
licença Apache-2.0 deste repositório nem por qualquer outra licença que a Mexerica possa conceder. Servem só para
**identificar o serviço** que o usuário escolhe conectar (uso nominativo), sempre ao lado do nome do serviço, sem sugerir
parceria, patrocínio ou endosso da Mexerica pelo fornecedor nem o inverso. Nenhum arquivo é usado como ícone do app, de
loja ou de material de marketing. Se um titular pedir a retirada ou as regras de marca dele mudarem, o arquivo sai e o
serviço volta ao globo. Fornecedor que publica regras próprias de uso de marca (por exemplo Slack, Google, Microsoft e
Meta) tem essas regras como as que valem; antes de usar a marca fora da identificação do conector (divulgação, loja, site),
conferir a política do titular. Ver também [TRADEMARK.md](TRADEMARK.md).

| Serviço | Arquivo | Origem (baixada em 2026-11-17; de Context7 em diante, em 2026-11-18) | SHA-256 |
| --- | --- | --- | --- |
| Linear | `linear.svg` | https://linear.app/static/favicon.svg?v=2 (ícone oficial do site; página de marca: https://linear.app/brand) | `6d49f8079f8cf91617657bd059ba1e186f2d3ecdcd9a4a366e0a6d5071c558e6` |
| Dono A | `dono-a.svg` | `/logo_bk.svg` do site oficial do serviço da ficha `host/src/fork/conectores/fichas/ficha-dono-a.json` (mesmo domínio das fontes da ficha; é o mesmo desenho do ícone de aba do site) | `f49e31c31ad28a5a033e741b7754f0808b1cb715d76b1bbd96b1e5e6c548c8d7` |
| Composio | `composio.png` | https://composio.dev/apple-touch-icon.png (ícone oficial do site) | `924f588685ca9887ccf937952878404da76837b6297884731bb9b175879e6548` |
| Slack | `slack.png` | https://a.slack-edge.com/80588/marketing/img/icons/icon_slack_hash_colored.png (ícone da Slack nos materiais do próprio site; página de marca: https://slack.com/media-kit) | `cfd9f050bf26ea547af438722c5a93be002c721f0f32a3c996595eab7af1b36d` |
| Gmail | `gmail.png` | https://fonts.gstatic.com/s/i/productlogos/gmail_2020q4/v8/web-64dp/logo_gmail_2020q4_color_2x_web_64dp.png (logo de produto publicado pelo Google) | `860f3d32b1733bf7ca01c827b3e032ca0e55c8d4fa0d8482395e333f4e42ef9c` |
| Google Drive | `google-drive.png` | https://fonts.gstatic.com/s/i/productlogos/drive_2020q4/v8/web-64dp/logo_drive_2020q4_color_2x_web_64dp.png (logo de produto publicado pelo Google) | `11d3364e146a17cedd74c6a76d70f0b33b1d74a9136525e042d86d53acc56918` |
| Google Agenda | `google-agenda.png` | https://fonts.gstatic.com/s/i/productlogos/calendar_2020q4/v13/web-64dp/logo_calendar_2020q4_color_2x_web_64dp.png (logo de produto publicado pelo Google) | `3834801ccb9e2fccaf81912e3fb46772b94673ff60624f340ba30ede23805f2c` |
| Outlook | `outlook.png` | https://res.cdn.office.net/assets/mail/pwa/v1/pngs/apple-touch-icon.png (ícone do Outlook na web, servido pela Microsoft) | `2344bf99eba344e5340c3a4ffca47a65ca036e7d5764e4f1fbcb8179d0fc11d4` |
| Instagram | `instagram.webp` | https://static.cdninstagram.com/rsrc.php/yr/r/rzWiSjZRxk5.webp (ícone do site oficial, declarado no <link rel="icon"> de https://www.instagram.com/) | `5ea73220ab5bda046e21d3f0cba26eafadeb3cc2bbd07a91978774048c84b6ee` |
| Context7 | `context7.svg` | https://context7.com/brand (kit de marca/imprensa do fornecedor). Regra de uso: Página Brand Assets do Context7; sem regra escrita além dos arquivos. | `e057ac467afe0d9177a0ca6393226368dea8fdea427e52a82f20286070a2b84a` |
| Exa | `exa.svg` | https://exa.ai/brand (kit de marca/imprensa do fornecedor). Regra de uso: Brand Guidelines da Exa; usar o logo azul sem alterações. | `a59e80ffefefffe15172857ac631b4e5b710c1946045fa4b5ddbf2d42d3d2bcf` |
| Neon | `neon.svg` | https://neon.com/brand (kit de marca/imprensa do fornecedor). Regra de uso: Brand kit oficial da Neon: usar a variante de cor adequada ao fundo, sem distorcer. | `2b1aaef0b2de5d30b9a00778a0a9b9df2255be08a939a9521f3c93a46de521cf` |
| Wispr Flow | `wispr-flow.png` | https://wisprflow.ai/media-kit (kit de marca/imprensa do fornecedor). Regra de uso: Media kit oficial da Wispr Flow (Logo Kit.zip): sem SVG do símbolo no kit; usar a versão escura/clara conforme o fundo. Regra de uso não publicada além do kit. | `71fcb5a4fe0662abc2190de9c66f5fe47dc22d441672e46c3ff48949404aacc9` |
| GitHub | `github.png` | https://brand.github.com/foundations/logo (kit de marca/imprensa do fornecedor). Regra de uso: Brand Toolkit do GitHub: Invertocat só em preto/branco (ou cinza/verde em poucos casos); sem sombras, gradientes, sem distorcer; o Invertocat isolado só onde a marca já está estabelecida. Kit só em PNG/PDF. | `2a2f5cbcc74c7fa83c40127dd8b0e42c23f1157131aebe28492aa1ac27bbdc6d` |
| ClickUp | `clickup.svg` | https://clickup.com/brand (kit de marca/imprensa do fornecedor). Regra de uso: Brand Assets & Guidelines do ClickUp (e Trademark Guidelines): não mudar cor, não girar, não esticar, não remover o símbolo. | `a8b08e70df4a7d7c949a423b4f3caa04671e209617c8b73bc24cff846ae2e017` |
| Firecrawl | `firecrawl.svg` | https://www.firecrawl.dev/brand/firecrawl-wordmark.svg (kit de marca/imprensa do fornecedor, https://www.firecrawl.dev/press-brand; baixado em 2026-11-20). Regra de uso: Página Press & Brand do Firecrawl: não esticar, recolorir nem modificar o wordmark. Arquivo é o wordmark com letras grafite e a chama laranja, para fundo claro. | `7edcff63dfe51f3129d039110786da574a8610ee0ed5998666f4663f3d744ae9` |
| Render | `render.svg` | https://render.com/press (kit de marca/imprensa do fornecedor). Regra de uso: Brand Kit oficial da Render (inclui _Render-Logo-Guidelines.pdf). Versão preta do logomark para fundo claro. | `c5f3790ed18ab18658cffae2bab49cc7e699955f534de013107f14f2f8398aed` |
| PlanetScale | `planetscale.svg` | https://planetscale.com/brand/planetscale-logo-mark-black.svg (kit oficial, baixado em 2026-11-28). Símbolo para espaço reduzido, usado ao lado do nome do serviço. | `f15c3ee6cbf65c2324ab73b6f36c376aa55a303ebccab42e26a14c8ac1155c2d` |
| Supabase | `supabase.svg` | https://supabase.com/brand (kit de marca/imprensa do fornecedor). Regra de uso: Brand Guidelines do Supabase: não alterar cores, forma nem proporção; manter espaço livre. | `eb9ffcd100ad26228a578100ba7cbfc5b383e4995338d122abdc6a3b4d210ed3` |
| Todoist | `todoist.svg` | https://www.todoist.com/press (kit de marca/imprensa do fornecedor). Regra de uso: Kit de imprensa oficial com Todoist Brand Guidelines.pdf (dentro do zip): ler o PDF antes de uso em material público. | `9adff05124fc2e773bcf4c7fff454ce341899d1806782dee543405bd77d53383` |
| Lovable | `lovable.png` | https://lovablebrand.lovable.app/brand/logo (kit de marca/imprensa do fornecedor). Regra de uso: Brand Hub da Lovable: cores aprovadas, sem esticar, sem efeitos, sem fundo poluído. Só PNG (2160x2198); sem SVG oficial. O arquivo do kit (2160 px) foi apenas reduzido a 256 px, sem recorte nem recolorização. | `dc0cb097d416f6207a9fedddc98ebcf233986726316be51935b4541fed368237` |
| Metricool | `metricool.png` | https://metricool.com/wp-content/uploads/cropped-web-app-manifest-512x512-1-180x180.png (ícone oficial do site, declarado no <link rel="apple-touch-icon"> de https://metricool.com/; o site não publica kit de marca) | `2c8004a9b64b73d7d4a8de60f3703f248db67820e539c2c1e7cf99d74116905f` |
| Short.io | `short-io.png` | https://short.io/favicon.png (ícone oficial do site, declarado no <link rel="icon"> de https://short.io/; o site não publica kit de marca) | `7367b2a4fb3d9c8fe36d55121fe9de80fe437caf55767dbd39ee72d595e5786f` |
| Airtable | `airtable.png` | https://airtable.com/company/trademark-guidelines (diretrizes de marca; o arquivo é o "Full color logo (primary)" da tabela oficial "Airtable Logos" ligada nelas, baixado em 2026-11-18). Regra de uso: o logo não pode ser modificado, distorcido nem redesenhado, e o titular precisa ser citado: Airtable e a marca Airtable são marcas registradas da Formagrid Inc, dba Airtable. Arquivo sem nenhuma alteração. | `12833c437e7a7d0c1e6c6547fd47cc150590bf6794e6750cb5e9fbc4a2f49f89` |
| Atlassian (Jira e Confluence) | `atlassian.svg` | https://www.atlassian.com/company/news/press-kit (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: O press kit oficial diz que os logos e lockups oficiais da Atlassian para programas, propriedades e produtos ficam na biblioteca de logos (atlassian.design/resources/logo-library); a página da biblioteca não entregou texto legível sem JavaScript. Logo obtida do SVG oficial do próprio site atlassian.com, uso nominativo para indicar integração, sem alterar cores ou forma. | `9537c3c727e6b7ddf30a2c6846b6b2cbbec6f8742380640f0323db7d3ee5ed2e` |
| Attio | `attio.svg` | https://attio.com/press (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Guidelines e assets para citar o Attio em cobertura de imprensa e materiais de terceiros (logo pack em SVG/PNG, variantes claro/escuro) | `56d1f7a7c43b0b28e576531fa1b5d260a1ed654f1e5ce18507db6fb8e6906e6e` |
| Brevo | `brevo.svg` | https://www.brevo.com/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial, uso nominativo | `b6a8f454586580a1d79f86cb10b694f316c6bcc0af6e8b88d813d23d85f81f67` |
| Browser Use | `browser-use.svg` | https://docs.browser-use.com (logo servida pelo CDN oficial Mintlify do site de docs) (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (docs), uso nominativo | `d6de051064eea8cdbe948b15082a552ec5ac319fed8e9cb92a7b8bfc001b9c5c` |
| Cal.com | `cal-com.svg` | https://cal.com/brand (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Use os assets e guidelines para representar o Cal.com adequadamente (brand kit disponível em cal.com/brand com zip de assets) | `449335d3618b314f15b07225057e44721697c0b22c89769f794c55249e1ac2ef` |
| Calendly | `calendly.svg` | https://calendly.com/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca imprensa (calendly.com/press) não publica kit nem regras legíveis; logo (símbolo) obtida do arquivo SVG oficial servido pelo próprio site Calendly, uso nominativo para indicar integração, sem alterar cores ou forma. | `ef2004d8cc8278f8036e3a0a3f57aa1ce11a28289aa64f16356ae74712b7bcfd` |
| Camtasia (TechSmith) | `camtasia.svg` | https://camtasia.techsmith.com/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial do Camtasia, uso nominativo | `07c2ceaef2cb23430803ddd9593a738d4e36e5e72f5e4b7ff92c716b023ac987` |
| Cloudflare | `cloudflare.svg` | https://www.cloudflare.com/brand/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Brand guidelines oficiais: usar as cores oficiais do logo (logomarca laranja tangerina #F6821F e manga #FBAD41, logotipo preto) sem alterar cores; em fundos coloridos usar a versão branca; espaço livre mínimo de 100% da altura da logomarca e tamanho mínimo de 100 px de largura na versão horizontal. Logo baixada do próprio site oficial cloudflare.com. | `c9ec1d5d5e4b0924666756a70179395c26b010372f8d7eec3539575281cebf1b` |
| Coda (Superhuman Docs) | `coda.png` | https://coda.io/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `4fc7c9baa8c1e306cbdcff58e039b7b58c6931e17c8204caa5489393428ad980` |
| Descript | `descript.svg` | https://static-cdn.descript.com/descript-website/images/descript-nav-logo-light.svg (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo (wordmark) tirada do site oficial, uso nominativo | `879e70492770525e8a0c92d7dcdb8c02cbfff6331a2165f2b18924058ddcd5b8` |
| Expo | `expo.png` | https://static.expo.dev/static/brand/app-icon-512x512.png (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo (app icon oficial do brand kit do site), uso nominativo | `d4e7fddf65a27109120b4c1c2ba66741ad9974dcbee3be65244b27b794c20f70` |
| Fal AI | `fal.png` | https://fal.ai (favicon oficial do site) (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `3f9867ae04060e02e05ee86447693702aae87dc68354cd664d3ecea44ff4097f` |
| Granola | `granola.svg` | https://www.granola.ai/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Não há página de marca ou press kit público (granola.ai/brand e /press retornam 404); logo extraída do SVG oficial do cabeçalho do site Granola, uso nominativo para indicar integração, sem alterar cores ou forma. | `de41e1d214b70b8ca349197f1a6169b7cc90e98dff701b52f1bead345ea4af3b` |
| Higgsfield | `higgsfield.png` | https://higgsfield.ai/icon.png (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `77edee005ae63f7881f930b815392b6cfa607f960ffb419d1c9df52e37fd50c1` |
| Jungle Scout | `jungle-scout.svg` | https://www.junglescout.com/press/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial (wordmark SVG), uso nominativo | `c17ab51911d50143325cc5518f0770a7de96e283100e99ece7ccf7cd5516969c` |
| mem0 | `mem0.svg` | https://docs.mem0.ai/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial de documentação, uso nominativo | `e03d0e3ca82aae25268abc15e0fbc84932d51c19a2d5134ae9033fb1a4cc4e54` |
| monday.com | `monday.svg` | https://monday.com/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Não foi encontrada página de marca ou press kit pública (monday.com/brand, /press e /press-kit retornam 404); logo (símbolo oficial) obtida do SVG do próprio site monday.com, uso nominativo para indicar integração, sem alterar cores ou forma. | `5b960ad220d6109e9d3388dbda51877cc2bf7c549000f9b0ace6291e97ea41f8` |
| Notion | `notion.png` | https://www.notion.so/images/logo-ios.png (ícone oficial do site, baixado em 2026-11-18; página de marca: https://notion.notion.site/Logos-6331ef1fc18241f8bae8a4b923b05ce4). Regra de uso: a página de marca oferece o bloco (block) para espaços pequenos, como ícone de rede social ou favicon, e variantes mono; usado o bloco, sem nenhuma alteração do arquivo. | `59c42a0afd20dc9dd0cf598e105e774a484bf10cdbec1ffc3a33c9cb59579b47` |
| OpenRouter | `openrouter.png` | https://openrouter.ai/apple-touch-icon.png (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (apple-touch-icon), uso nominativo | `ec915bd303e5d8501f02a37029fa01ede150b507357ef4c9739efb720d0df445` |
| PayPal | `paypal.svg` | https://www.paypal.com/us/webapps/mpp/logo-center (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: O centro de logos oficial da PayPal bloqueia acesso automatizado (HTTP 403), então as regras de uso não puderam ser lidas; logo (wordmark) baixada do CDN oficial paypalobjects.com usado pelo próprio PayPal, uso nominativo para indicar integração, sem alterar cores ou forma. | `f766ba6d9471acc787c0808b8c30f38494d12b287ccfb2ff610fd617cfa2a432` |
| Pipedrive | `pipedrive.png` | https://www.pipedrive.com/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial (apple-touch-icon), uso nominativo | `a7a36b1b088b2e582f3a9dc4587b3c50c0c22242bb73abf8ca3363ab104bf872` |
| Plaud | `plaud.svg` | https://docs.plaud.ai/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Não há página pública de marca ou press kit localizada; logo oficial (lockup Plaud) obtida do tema da documentação oficial da Plaud (mintcdn servido por docs.plaud.ai), uso nominativo para indicar integração, sem alterar cores ou forma. | `eb7fc0b5d8ef961c858d912b5e3a558729faa18c4b74967622c25b3c269e7ce5` |
| Railway | `railway.png` | https://railway.com/brand/logo-dark.png (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (railway.com/brand/logo-dark.png), uso nominativo | `c1dab72a00129e66d40b6989bf41840f9f79b03f88e52ffd43d2590e7e3000af` |
| Resend | `resend.png` | https://resend.com/static/favicons/favicon-marketing@180x180.png (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `71ab800939b1c3db40136b2c66f5d76eb840e72f77728e55f3cb1092ef91948a` |
| Semrush | `semrush.svg` | https://www.semrush.com/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `c71deb8dd523399cef29496fa350a942d7d0445dcad0fea398818f474d5208db` |
| Sentry | `sentry.svg` | https://sentry.io/brand/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca oficial existe, mas o texto carrega por JavaScript e não foi legível sem navegador; logo extraída do CDN oficial do próprio site Sentry (mask-icon logo-sentry.svg), uso nominativo para indicar integração, sem alterar cores ou forma. | `4131091b0100dc2c1346672261c0de5d1ae8f6aa4092ff518f6046f149089b47` |
| Stripe | `stripe.svg` | https://stripe.com/newsroom/brand-assets (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Kit oficial de logos do Stripe Newsroom: o wordmark vem em três cores — usar slate e blurple em fundo claro e branco em fundo escuro; não usar outra cor. Marcas e logos sujeitos ao Stripe's Marks Usage Agreement. | `4448c4b4f954285d2b2aeb6d92391c85fdc290e008c2679d2c006d6d72ae1ae9` |
| Tally | `tally.svg` | https://tally.so/favicon.svg (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `fefba3256dcca1455cc4db0076dd3e9494c50edb6768b4a15ea8c9f5ea10d408` |
| TikTok Ads (TikTok for Business) | `tiktok-ads.png` | https://business-api.tiktok.com/portal/docs (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Página de marca não encontrada: logo tirada do site oficial (favicon do portal de desenvolvedores da Business API), uso nominativo | `5e9c735171956561f39711ea2c6b593ad1d288e53cad060237d0f986202ffb36` |
| TinyFish | `tinyfish.svg` | https://www.tinyfish.ai/ (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: Não há página pública de marca (tinyfish.ai/brand retorna 404), mas o site serve um pacote oficial de logos em /TF-Logos; usada a versão horizontal oficial TF_Horizontal.svg, uso nominativo para indicar integração, sem alterar cores ou forma. | `09e9a90125aabe330dd84a1fb124ab47e6cda7b6e30e2b9b027e6d0865cdb240` |
| vidIQ | `vidiq.png` | https://vidiq.com/favicons/apple-icon-180x180.png (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (favicon), uso nominativo | `2dc2ec6330afb8bd9f26d98db3cc9fb6e90c4060fde6eb85bc4109f39b30d97f` |
| Wix | `wix.png` | https://www.wix.com/favicon.ico (fonte oficial do fornecedor, baixada em 2026-11-18). Regra de uso: página de marca não encontrada: logo tirada do site oficial (favicon 192px), uso nominativo | `295a1f6f927fd11a3842a4c9f508b4152eca150cb4c54d6cfb64736fad659b80` |
| HeyGen | `heygen.png` | https://www.heygen.com/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `2fe89e666625c4c96ce25f94f18ae74a73c2abd626b0b594287dd35f2ebef4d9` |
| Apify | `apify.png` | https://apify.com/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `c0e4f43da0098f9f2a5df020a875f52c0ad34579e1f308c7db71ac396f20d6e8` |
| Tavily | `tavily.svg` | https://www.tavily.com/icon.svg (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem alteração ou endosso. | `c265bd72bed3b8c509a4e3abdd378f4462ef26ff4a3c8789b5aa88c2f6107b65` |
| Trello | `trello.png` | https://trello.com/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `c6567e3202ec7e1aed0dacdf079aefdef7e2c5c2c03b6a9fe8ee355896dc26d6` |
| AgentMail | `agentmail.png` | https://www.agentmail.to/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `c0daa69a855ddef51b1a1d9f7953726fc9b4c40c3603207e2f6c0c7e149c4581` |
| Bright Data | `bright-data.png` | https://brightdata.com/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `64f3340ce88a671f2468269da5526f55c67a2637c5fcd2b3a6f294251a7e2a08` |
| Prisma | `prisma.png` | https://www.prisma.io/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `8bb3224ccae71e25841b163608617f33bceb09e518b472f8dfbd2bd2eecf88fc` |
| Wrike | `wrike.png` | https://www.wrike.com/favicon.ico (ícone do site oficial, baixado em 2026-11-27). Uso nominativo, sem endosso; maior quadro do favicon, em PNG sem recorte nem recolorização. | `2b2027ed056dcae54d079fc52cc0eaacfa55c3c4459c7a088298e21995c37bfa` |

| Better Stack | `better-stack.svg` | `app_icon.svg` no [kit oficial](https://static.betterstackassets.com/brand/better_stack_logos.zip), baixado em 2026-11-27. [Diretrizes](https://betterstack.com/docs/brand/): manter proporções, cores e arte original, sem sugerir endosso. | `f85d2cfc6116ab257b2213687a903a8231e53933d896b73d9e729733d57a5209` |
| Clerk | `clerk.svg` | [Símbolo primário](https://clerk.com/v2/downloads/symbol-primary.svg) do [kit oficial](https://clerk.com/brand-assets), baixado em 2026-11-27. Uso nominativo, sem alteração. | `e176af85b4fbdece5352ed2fb641bf6f8cb591ad84f72d605d283a829e3c12a1` |
| CodeRabbit | `coderabbit.svg` | [Símbolo laranja](https://www.coderabbit.ai/content/assets/orange-logomark.svg) do [kit oficial](https://www.coderabbit.ai/brand), baixado em 2026-11-27. Usar o símbolo quando o logotipo completo não couber, sem recorte nem alterações. | `25c8cd16561dbad015504d2b2c2ed9eb86a49956c91af161bdc9aa05499336ee` |
| PostHog | `posthog.svg` | [Ícone oficial](https://brand.posthog.com/favicon.svg) do [site de marca](https://brand.posthog.com/logo), baixado em 2026-11-27. [Diretrizes](https://posthog.com/handbook/brand/assets): usar o símbolo em tamanho pequeno sem modificar a arte. | `4bb46371b6b201482bd009d77cf2cf41fa1246e0581130292924e389c410fe56` |
| RevenueCat | `revenuecat.svg` | [Ícone do site oficial](https://www.revenuecat.com/favicon/favicon.svg), baixado em 2026-11-27. Uso nominativo, sem alteração. | `03648f206cb744862113718d5fbe33e86de0f5478c0030f976a220561844af1b` |

O dono A não publica uma página de marca; o arquivo é o logo que o site oficial dela serve. Em caso de dúvida do titular,
vale o pedido dele (ver acima).

## Avisos distribuídos nos instaladores

Os arquivos abaixo são copiados para dentro de cada instalador (pasta `resources/avisos/` do app) pela configuração
`extraResources` de `apps/desktop/electron-builder.yml`: `LICENSE`, `NOTICE`, `THIRD_PARTY.md`, `TRADEMARK.md`,
`MODIFICACOES.md`, `MODIFICACOES-ARQUIVOS.md`, `brand/LICENCA-DA-ARTE.md` (como `avisos/LICENCA-DA-ARTE.md`), os textos OFL das fontes, e os arquivos de `packaging/avisos/` descritos aqui.
**Esta configuração não foi exercitada num empacotamento real** nesta etapa (o binário do Electron não é baixado no ambiente de
revisão); quem gerar o primeiro instalador deve conferir que `resources/avisos/` aparece no pacote de cada sistema.

### Electron e Chromium

O app desktop roda sobre o Electron 44.11.4 (MIT; copyright dos contribuidores do Electron e da GitHub Inc., como no texto da licença). O texto da licença
está em `packaging/avisos/electron-LICENSE.txt` (cópia do arquivo `LICENSE` do pacote npm `electron`). O Electron embute o Chromium e
dezenas de bibliotecas de terceiros, cujos avisos o projeto publica no arquivo `LICENSES.chromium.html`, que acompanha o
binário do Electron baixado no empacotamento. **Não há prova nesta etapa de que esse arquivo entre no `.app` do Mac**; essa
conferência fica pendente para a etapa de geração de instaladores e atualização e deve ser feita antes de distribuir um
instalador.

### WebRTC (helper de tela do Mac)

O helper de tela do Mac (`apps/screen-macos`, projeto `apps/Mexerica.xcodeproj`) usa o pacote `stasel/WebRTC` na versão exata 153.0.0
(Swift Package; fixação em `apps/Mexerica.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved`, revisão
`4266157cd08f92115de885ab12d87196a8db87e1`), que redistribui o WebRTC do Google compilado. O texto da licença desse pacote (BSD-3-Clause,
com os avisos do WebRTC do Google) está em `packaging/avisos/webrtc-stasel-LICENSE.md`, copiado do repositório
`https://github.com/stasel/WebRTC` nessa revisão.

### Dependências de produção

`packaging/avisos/INVENTARIO-DEPENDENCIAS.md` lista as dependências de produção do app Electron e do Motor (a partir do `package-lock.json`),
os crates de produção e de compilação do Hub (a partir do `Cargo.lock`) e as bibliotecas que o empacotador do app embute no código do
renderer mesmo quando o lock as marca como de desenvolvimento (React, React DOM, Scheduler, Lucide, QRCode, dijkstrajs, xterm e o código de
suporte do Vite; medido num build Linux, não num instalador). **Cobertura e limites estão no começo do inventário.** Ele não lista tudo
o que um instalador leva: não cobre o Electron e o Chromium, as bibliotecas nativas de macOS e Windows, o helper do Mac nem
os assets que não vêm do npm, e traz só o identificador SPDX declarado por cada pacote, sem o texto integral de cada licença nem os avisos
de copyright de cada autor. Para distribuir um instalador, esses textos e avisos ainda precisam ser reunidos e conferidos contra o pacote
real, antes de distribuir. Licenças permissivas (MIT, Apache-2.0,
BSD, ISC, Zlib, Unicode) são a grande maioria. Pontos de atenção: o crate `option-ext` (MPL-2.0, licença de arquivo: os arquivos do
crate são usados sem modificação e seu código-fonte está público em crates.io) e o pacote `argparse` 2.0.1 (Python-2.0, licença permissiva da Python Software Foundation,
dependência de produção do app Electron). Os crates `r-efi` oferecem MIT ou Apache-2.0 além de LGPL; vale a opção permissiva.

### Helper de tela do Linux (apps/screen-linux)

Dependências diretas declaradas em `apps/screen-linux/Cargo.toml`, com as versões fixadas em `apps/screen-linux/Cargo.lock` e as licenças conferidas nos metadados do crates.io em 22/11/2026 (não copiadas no repositório; baixadas pelo Cargo na compilação). O grafo transitivo completo de `apps/screen-linux/Cargo.lock` e os textos integrais de aviso ainda precisam ser consolidados em `packaging/avisos/` antes de distribuir um pacote Linux com o binário compilado.

| Crate | Versão (`Cargo.lock`) | Licença |
| --- | --- | --- |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 |
| `ashpd` | 0.13.13 | MIT |
| `atspi` | 0.30.0 | Apache-2.0 OR MIT |
| `base64` | 0.22.1 | MIT OR Apache-2.0 |
| `dirs` | 7.0.0 | MIT OR Apache-2.0 |
| `futures-util` | 0.3.34 | MIT OR Apache-2.0 |
| `gstreamer` | 0.25.4 | MIT OR Apache-2.0 |
| `gstreamer-app` | 0.25.2 | MIT OR Apache-2.0 |
| `gstreamer-sdp` | 0.25.3 | MIT OR Apache-2.0 |
| `gstreamer-webrtc` | 0.25.4 | MIT OR Apache-2.0 |
| `rustix` | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 |
| `tokio` | 1.53.3 | MIT |
| `tracing` | 0.1.44 | MIT |
| `tracing-subscriber` | 0.3.23 | MIT |
| `zbus` | 5.19.0 | MIT |
