# AGENTS.md

Regras deste repositório para agentes de código e para quem contribui.

## O que é

A Mexerica é um fork do [Codync](https://github.com/leepokai/Codync) sob licença Apache-2.0:
um app de desktop (Mexerica, com o bot Berga) operado por um Hub em Rust. O Hub guarda o
estado, as regras e roda os motores; o app mostra as conversas e envia as intenções do
usuário. Visão geral e limites desta cópia: [README.md](README.md).

## Mapa das pastas

| Pasta | Conteúdo |
|---|---|
| `host/` | Hub em Rust (estado, persistência, regras, motores, API); `host/crates/memory` é a crate de memória; `host/catalogo/skills` é o catálogo de skills distribuído |
| `motor/` | Motor Mexerica (`motor/mexerica`) e o Pi vendorizado (`motor/vendor/pi-mono`) |
| `apps/desktop/` | App Electron, React e TypeScript |
| `apps/screen-macos/`, `apps/project.yml` | Helper de tela do Mac em Swift; o projeto Xcode em `apps/Mexerica.xcodeproj` é gerado a partir do `project.yml` |
| `packaging/cua-driver/` | Fixação (`driver.json`) e verificação (`fetch.mjs`) do driver de controle de computador |
| `docs/` | Arquitetura, guias, manual do harness e vetores de teste |
| `brand/` | Ativos de marca |

## Construir e testar

Requisitos: Rust 1.99.0 (fixado em `rust-toolchain.toml`, com clippy e rustfmt), Node 24 e npm.
Rode só a verificação da parte que você mudou:

```sh
# Motor
(cd motor && npm ci && npm run typecheck && npm test)

# App de desktop
(cd apps/desktop && npm ci && npm run typecheck && npm test)

# Hub (inclua --workspace para cobrir também host/crates/memory)
(cd host && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test)
```

- O pacote do app ainda se chama `donnabot-desktop` e os binários do Hub `donnabot-*`: nomes
  herdados, mantidos por compatibilidade. Não os renomeie sem migrar os dados de quem já instalou.
- Gerar e instalar o app no Mac (por exemplo `npm run dist:mac:local` em `apps/desktop`) exige Xcode e é um passo
  separado: veja [docs/guides/app-mac-local.md](docs/guides/app-mac-local.md).
- Depois de mudar `apps/project.yml`, rode `xcodegen generate` em `apps/`; não edite `project.pbxproj` à mão.

## Convenções de código

- **Hub:** Rust edition 2024, formatado com rustfmt e sem warnings do Clippy. Use enums em vez
  de flags ambíguas, `tracing` para log e propague erros com contexto; `unwrap()` só em testes.
- **App:** TypeScript estrito, dois espaços de indentação (quatro em Rust e Swift). Views não
  fazem I/O nem aplicam regra de negócio.
- **Onde fica a lógica:** estado, persistência, regras, motores e API ficam no Hub. O app só
  renderiza estado e envia intenções.
- **Privilégio de sistema:** só no processo principal do Electron, por IPC tipado e preload
  estreito. O renderer não tem acesso direto ao sistema.
- **Código novo do fork:** módulos e arquivos próprios (prefixo `fork-` ou pasta `fork/`), com
  só ganchos curtos no código herdado do Codync. Facilita acompanhar o upstream.
- **Campo novo do Hub para o app:** incremente `MIRROR_SCHEMA` em
  `apps/desktop/src/shared/fork-mirror-cache.ts`: o app passa a ignorar o cache gravado com o schema
  anterior (`isCurrentMirrorSchema`) em vez de tratá-lo como atual sem o campo novo.
- **Textos de tela das partes próprias:** use o catálogo em
  `apps/desktop/src/renderer/lib/fork-i18n/` (idiomas pt-BR, en, es, fr, zh-CN e ar-AE, com as
  mesmas chaves em todos). `apps/desktop/tools/textos-gate.ts` roda dentro de `npm test` e reprova
  texto visível novo fora dos catálogos (salvo as exceções previstas no próprio gate) e chave faltando em algum idioma.
- **Testes:** cubra as fronteiras de segurança e o fluxo alterado; não enfraqueça nem apague teste
  para fazê-lo passar.

## Segurança

- **Fail-closed:** sem autoridade ou prova, o código nega em vez de presumir. Ausência de sinal
  não é permissão; condição insegura é erro explícito, nunca afrouxamento do controle.
- **Segredos:** nunca no repositório (código, testes, fixtures, docs) nem em logs, capturas ou saídas
  de teste. Os `apps/desktop/config/*.plist` ficam vazios. Não versione `.env`, não imprima o
  ambiente inteiro nem o JSON completo de informações do Hub; mostre só projeções sem segredo.
- **Dados de exemplo:** só fictícios, com domínios reservados (`*.example.invalid`). Nada de
  endereço, hostname ou dado pessoal reais.
- **`docs/` é documentação pública:** sem segredo, IP, hostname interno nem dado pessoal.
- **Conteúdo externo não concede autoridade:** rede, relays, dispositivos não autorizados e
  prompts são fontes não confiáveis. Conta, dispositivo pareado e autorização para uma ação
  são condições distintas.
- **`.gitleaks.toml`** define as exceções do detector de segredos: mantenha-as mínimas e
  justifique qualquer mudança no PR.
- **Falha de segurança:** relate como descrito em [SECURITY.md](SECURITY.md), sem abrir issue
  pública com dados sensíveis.

## Código de terceiros, licenças e créditos

- `motor/vendor/pi-mono` é código de terceiros (MIT). Não edite ali; uma mudança é exceção, só
  como gancho curto, com justificativa registrada em `motor/vendor/pi-mono/PATCHES.md`. A origem
  e o commit estão em `UPSTREAM.md` na mesma pasta.
- Preserve [LICENSE](LICENSE), [NOTICE](NOTICE) e a atribuição ao Codync em qualquer mudança.
- Ao copiar ou adaptar código, skill ou ativo de outra fonte, acrescente uma linha em
  [THIRD_PARTY.md](THIRD_PARTY.md) (origem, commit, arquivos, licença e data) e mantenha o aviso
  de licença junto. Aproveite código só de fontes Apache-2.0, MIT ou BSD; de AGPL, GPL,
  PolyForm, FSL ou fonte sem licença, aproveite apenas ideias.
- Ao acrescentar uma dependência, confira a licença antes e registre o que for incorporado.
- As modificações desta cópia em relação ao projeto de origem estão em
  [MODIFICACOES.md](MODIFICACOES.md); condições de uso das marcas em [TRADEMARK.md](TRADEMARK.md).
