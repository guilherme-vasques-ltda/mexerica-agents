# Mexerica Agents

Hub em Rust, Motor e app de desktop para Mac de uma assistente pessoal com bots, conversas e ferramentas.
O **Hub** guarda o estado, as regras e roda os motores; o **app de desktop** mostra as conversas e envia o que você escolher; o **Motor** (baseado no Pi, vendorizado) executa o laço do agente preso ao Hub.

> **Estado:** em desenvolvimento. O app abre, mas a conversa com os bots ainda não funciona nesta edição.
> Não há instalador, versão publicada, serviço hospedado ou conta conectada. Nada aqui comprova instalação
> ou uso em máquina real: isso precisa ser verificado por quem for usar. Esta cópia não promete
> compatibilidade com plataformas Apple além do que for testado em cada versão.

## O que está aqui

| Caminho | Conteúdo |
|---|---|
| `host/` | Hub (Rust): estado, persistência, regras, motores e API; crate de memória; catálogo de skills distribuído |
| `motor/` | Motor Mexerica e o Pi vendorizado (`motor/vendor/pi-mono`, MIT) |
| `apps/desktop/` | App Electron/React/TypeScript e operações locais do dispositivo |
| `apps/screen-macos/`, `apps/project.yml`, `apps/Mexerica.xcodeproj/` | Helper nativo de tela do Mac (Swift) e seu projeto Xcode |
| `packaging/cua-driver/` | Fixação e verificação do driver de controle de computador (Cua, MIT), buscado na hora de gerar o pacote |
| `brand/` e arte em `apps/desktop/` | Arte própria da Mexerica (mascote, Mexeriquinhas, ícones, logo, fundos e personagens), com a regra de uso em [brand/LICENCA-DA-ARTE.md](brand/LICENCA-DA-ARTE.md) |
| `docs/` | Arquitetura, guia do app no Mac, manual do harness e vetores de teste |

## O que não está aqui

- Código de nuvem e relay hospedados, versão web e demo web.
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
