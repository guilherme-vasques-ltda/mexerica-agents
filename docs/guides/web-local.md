# Mexerica Web local

A versão Web completa do Mexerica Open, servida pelo próprio Hub na mesma origem
(`http://127.0.0.1:<porta>`), sem site institucional, sem demo pública, sem conta em nuvem.
O app é o mesmo renderer do desktop; a página nasce no portão de acesso local.

## Pré-requisitos

- Rust 1.99 (fixado em `rust-toolchain.toml`) e Node 24 com npm.
- Binário do Hub compilado na forma padrão: `cargo build --bin donnabot-host` na pasta
  `host/`, sem a feature `open` (ela monta uma build somente-socket-Unix, sem `--web-dir`,
  que não serve a Web local).
- Build web pronta: `npm run build:web` em `apps/desktop/`.

## Subir com um comando

```sh
cd apps/desktop
npm run web
```

O script localiza o binário (`MEXERICA_HOST_BIN`, `host/target/release/donnabot-host` ou
`host/target/debug/donnabot-host`), inicia `donnabot-host serve --bind 127.0.0.1 --port
<porta> --web-dir dist/web`, espera o `/health` responder e abre o navegador.

Flags opcionais:

- `--port <n>`: porta do Hub e da Web (padrão `19423`).
- `--build`: roda `npm run build:web` antes de subir.
- `--no-open`: não abre o navegador.

## Destravar no navegador

O Hub salva um token local em arquivo neste computador — por exemplo `~/.mexerica/token`
(ou `~/.mexerica-dev/token` em build dev). Abra `http://127.0.0.1:<porta>/`, cole o conteúdo
desse arquivo no portão de acesso e pronto: a sessão vira um cookie `HttpOnly` do Hub. O
token nunca aparece no endereço do navegador e o script `npm run web` nunca lê nem imprime o
conteúdo dele — só aponta o caminho do arquivo.

Se a sessão expirar com o app aberto, a página recarrega sozinha e volta limpa ao portão
(fail-closed): nada do estado do app fica para trás.

## Segurança

- **Só `127.0.0.1`:** o Hub aceita conexões apenas do próprio computador; nada é publicado.
- **Sessão `HttpOnly`:** o cookie de sessão nasce na resposta de login do Hub e o JavaScript
  nunca o vê; o token digitado não vai para `localStorage`, `sessionStorage` ou URL — o CSRF
  token e os metadados ficam só em memória. Navegadores não isolam cookies por porta em
  `127.0.0.1`: outro servidor local visitado no mesmo navegador pode receber o cookie de sessão
  (embora não receba o `X-Mexerica-Csrf` mantido apenas em memória). Evite executar serviços
  não confiáveis em outras portas de `127.0.0.1` na mesma sessão de navegador.
- **CSRF e streams:** toda chamada `POST /api/<method>` e todo stream leva o header `X-Mexerica-Csrf`;
  o Hub recusa sem ele e encerra streams abertos (`/events`, `/term/*`) assim que a sessão é
  revogada no logout, descartada pelo limite de sessões ou expirada.
- **CSP estrita:** o `index.html` declara `script-src 'self'` sem script inline (nem hash nem
  `unsafe-inline`), `connect-src 'self'` (o app só fala com o Hub da mesma origem) e
  `object-src 'none'`. A build reprova se aparecer URL externa de autenticação ou script
  externo no bundle.
- **Fail-closed:** sem sessão, a ponte recusa chamadas ao Hub antes de sair do navegador.

## O que a Web não tem (degradação explícita)

Recursos nativos de desktop/SO ficam desligados e o app avisa em vez de fingir:

- **Captura nativa de tela** (`screenAgentState`): indisponível.
- **Bandeja (tray)**: sem menu na bandeja; `setTraySummary` é um no-op.
- **Ilha (notch do Mac)**: `supported: false`.
- **Fala nativa do sistema** (`speech`): `available: false`.
- **Cofre seguro local** (`forkCofre`): `semSuporte`.
- **Atalho global da chamada de voz**: não existe no navegador.

Downloads de arquivos compartilhados por bots funcionam: os pedaços viram um Blob no
navegador e o download sai pelo diálogo padrão (limite de 100 MiB, igual ao desktop). A
integridade SHA-256 do arquivo é verificada antes de salvar — pedaço errado ou corrompido
não vira download.
