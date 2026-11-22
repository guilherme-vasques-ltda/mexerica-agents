# Mexerica Agents

Hub em Rust, Motor, app de desktop, versão Web local e clientes móveis de uma assistente pessoal com bots, conversas e ferramentas.
O **Hub** guarda o estado, as regras e roda os motores; o **app de desktop**, a **versão Web local** e os **clientes móveis** mostram as conversas e enviam o que você escolher; o **Motor** (baseado no Pi, vendorizado) executa o laço do agente preso ao Hub.

> **Estado:** em desenvolvimento. O [guia do Mexerica Open](docs/guides/app-mac-open.md) descreve conversa local
> por texto no macOS com um Codex CLI compatível e login local configurado; o [Motor Mexerica não
> executa nesse sistema](docs/features/motor-mexerica.md). A disponibilidade dos demais motores depende
> da plataforma e da configuração.
> Ainda não há versão publicada, instalador público para download ou serviço hospedado; nenhuma conta
> vem pré-configurada. Há guias para [compilar o app no Mac](docs/guides/app-mac-local.md) e [instalar
> o Hub no Linux](docs/guides/instalacao-hub-linux.md). Esses fluxos locais não validam instaladores de
> distribuição nem login ou conversa com conta real. Não use com contas ou dados reais:
> nesta edição, os motores de terceiros que agem sem pedir aprovação foram retirados da oferta, e os
> motores oferecidos pedem aprovação antes de agir.
> Esta cópia não promete compatibilidade com plataformas Apple além do que for testado em cada versão.

## O que está aqui

| Caminho | Conteúdo |
|---|---|
| `host/` | Hub (Rust): estado, persistência, regras, motores e API; crate de memória; catálogo de skills distribuído |
| `motor/` | Motor Mexerica e o Pi vendorizado (`motor/vendor/pi-mono`, MIT) |
| `apps/desktop/` | App Electron/React/TypeScript, versão Web local (`src/web/`, [guia Web local](docs/guides/web-local.md)) e operações locais do dispositivo |
| `apps/screen-macos/`, `apps/project.yml`, `apps/Mexerica.xcodeproj/` | Helper nativo de tela do Mac (Swift) e seu projeto Xcode |
| `apps/screen-linux/` | Helper nativo de tela do Linux em Rust (portais xdg, PipeWire e GStreamer WebRTC) |
| `apps/mobile/` | Clientes móveis iOS e Android (Swift/Xcode e Kotlin/Gradle) |
| `packaging/` | Driver de controle de computador (`cua-driver/`, Cua, MIT), avisos de dependências (`avisos/`), ferramentas de atualização (`atualizacao/`) e backup do Hub (`hub-backup/`) |
| `brand/` e arte em `apps/desktop/` | Arte própria da Mexerica (mascote, Mexeriquinhas, ícones, logo, fundos e personagens), com a regra de uso em [brand/LICENCA-DA-ARTE.md](brand/LICENCA-DA-ARTE.md) |
| `docs/` | Arquitetura, guias (Mac, Web local, bastidor), manual do harness e vetores de teste |

## O que não está aqui

- Código de nuvem e relay hospedados (serviços em nuvem e versão web hospedada; a versão Web local e a demo local do desktop estão neste repositório).
- Logos de agentes de terceiros e o logotipo da Apple: o app mostra ícones genéricos. As logos dos serviços do catálogo de conectores estão incluídas, só para identificar cada serviço, e continuam sendo marcas de seus titulares (ver [TRADEMARK.md](TRADEMARK.md) e [THIRD_PARTY.md](THIRD_PARTY.md)).
- Arte de terceiros: os 43 ícones de agentes, o logotipo da Apple, `google.svg` e `computer-linux.svg` continuam neutros. Licenças e marca em [THIRD_PARTY.md](THIRD_PARTY.md) e [TRADEMARK.md](TRADEMARK.md). Da arte própria da Mexerica (ver "O que está aqui"), só as Mexeriquinhas e as 19 opções de ícone (`padrao.png` fora) são Apache-2.0; o resto da arte da Mexerica fica com os direitos reservados e só vale dentro da Mexerica (regra e caminhos em [brand/LICENCA-DA-ARTE.md](brand/LICENCA-DA-ARTE.md)).
- Configuração de serviços e segredos (chaves, endereços reais, contas).

## Como verificar (sem rede de produção)

Requisitos: Rust 1.99.0 (`rust-toolchain.toml`), Node 24, npm.

```sh
# Motor
cd motor && npm ci && npm run typecheck && npm test

# App de desktop (typecheck e testes unitários)
cd apps/desktop && npm ci && npm run typecheck && npm test

# Hub
cd host && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Gerar e usar o app no Mac (Xcode, assinatura local, permissões) é um passo separado, descrito em [docs/guides/app-mac-local.md](docs/guides/app-mac-local.md), e precisa ser conferido em um Mac de teste, com dados fictícios.

## Configuração

`apps/desktop/config/*.plist` vêm vazios: nuvem e login ficam desligados (falha fechada). Não coloque chaves, endereços reais ou dados pessoais neste repositório.

## Segurança

Veja [SECURITY.md](SECURITY.md).

## Licença, créditos e marca

Baseado no [Codync](https://github.com/leepokai/Codync) (Po Kai Lee e contribuidores), licença [Apache-2.0](LICENSE). Avisos em [NOTICE](NOTICE) e [THIRD_PARTY.md](THIRD_PARTY.md); modificações desta cópia em [MODIFICACOES.md](MODIFICACOES.md); condições de marca em [TRADEMARK.md](TRADEMARK.md). Sem endosso do projeto de origem.
