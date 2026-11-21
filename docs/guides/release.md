# Release do app

Versão da Mexerica → rascunho de GitHub Release com os instaladores. Release por
entrega, não por PR; o mantenedor aprova merge, disparo e publicação. Nada é
automático: o workflow nunca publica nem cria tag oficial.

## Versão

SemVer, política beta `0.MINOR.PATCH` — somente versões `v0.x`. No modo release, o
semver validado entra no `apps/desktop/package.json` da VM descartável antes de
empacotar, para app, `.deb`, `.AppImage` e NSIS reportarem a versão da rodada; os
binários do Hub mantêm a versão do `host/Cargo.toml`, preservando builds
`--locked`. O `CHANGELOG.md` é fonte humana: cada rodada acrescenta sua seção.

## Passos

1. PR de release na `main`: bump para `0.x.y` em `apps/desktop/package.json` +
   seção `## [0.x.y] - AAAA-MM-DD` no `CHANGELOG.md` (escrita por humano). Merge
   com CI verde.
2. Mantenedor dispara o workflow **Release** por `workflow_dispatch` a partir da
   `main`, com `mode: release` e `version: v0.x.y`. A versão é validada contra o
   `apps/desktop/package.json` (`tools/ci-release-version.mjs`) e a rodada só
   acontece em runners do GitHub com o repositório público.
3. O workflow compila o app Open do Mac e os pacotes Linux (`.deb`, `.AppImage`),
   gera as notas com `tools/ci-release-notes.mjs` (mesma trava contra bastidor do
   CI) e cria um **rascunho** `--draft --prerelease` no GitHub Releases apontando
   para o commit da rodada, com os seis arquivos e o `SHA256SUMS` no resumo.
4. Mantenedor revisa o rascunho e publica. O modo `preview` entrega só os
   artefatos no Actions, sem rascunho, para conferência prévia.

## Limites e reversão

- Só dispatch de `main`: o workflow confere o `GITHUB_REF`, recusa repositório
  privado e máquina fora do GitHub e falha fechado em qualquer erro de leitura.
- Limite de confiança: o dispatch roda a definição do workflow do ref escolhido.
  A checagem de `main` evita acidente, não escritor malicioso: quem tem escrita
  pode disparar uma cópia alterada na própria branch. O limite real é a lista de
  colaboradores com escrita.
- Rascunho não publicado: apague no GitHub Releases. Publicado por engano: nunca
  reutilize a versão, lance uma superior.
