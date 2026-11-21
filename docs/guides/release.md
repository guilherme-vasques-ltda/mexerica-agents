# Release do app

Versão da Mexerica → tag `v0.x` → GitHub Release em rascunho. Release por
entrega, não por PR; o mantenedor aprova merge, tag e publicação. Nada é
automático.

## Versão

SemVer, política beta `0.MINOR.PATCH` — **somente** tags `v0.x`. Fonte única:
`apps/desktop/package.json` == `host/Cargo.toml`, alterada só no PR de release
(nunca pelo script). O app já está em `0.1.0`; se um dia voltar à linha
herdada `2.x`, `release.mjs --check` reporta BLOQUEADO até nova migração.

## Passos

1. PR de migração/release na `main`: bump para `0.x` nos dois manifestos +
   seção `## [0.x.y] - AAAA-MM-DD` no `CHANGELOG.md` (escrita por humano).
2. Minuta opcional: `node scripts/release/notes.mjs collect --since REF
   --out DIR` lista PRs mergeados na main em (since, HEAD] por ancestralidade
   do merge commit; revise `release-draft.md`.
3. Merge com CI verde. Na `main` limpa:
   `node scripts/release/release.mjs --check --since REF` e depois
   `--create-tag v0.x.y --since REF --until HEAD --yes`. O `--until` deve
   resolver para HEAD: a tag só aponta para o commit com CI validado.
   Confere tag 0.x == versão, main, tree limpa, tag nova, fronteira e CI.
   Nunca faz push.
4. Mantenedor cria e publica a tag (`git tag -a`/`push`; o script já a cria
   local) e dispara o workflow **Release** por `workflow_dispatch` em `main`
   com o nome da tag; ele revalida tudo e cria o **rascunho** do CHANGELOG,
   que o mantenedor revisa e publica.

## Limites e reversão

- Sem anexos (DMG/ZIP): fase futura. Só notas+tag.
- Só dispatch de `main` (push de tag não dispara; proteção de tags não
  verificada). O workflow lê a tag, a main e o CI pela API do GitHub e falha
  fechado em qualquer erro de leitura; depois de criar o rascunho confere a
  tag de novo. Qualquer falha nessa reverificação encerra o workflow com erro e
  preserva o release para revisão e limpeza manual. O workflow nunca
  exclui releases, inclusive se o mantenedor publicar durante essa janela.
- Limite de confiança: o dispatch roda a definição do workflow do ref
  escolhido. A checagem de `main` evita **acidente**, não escritor malicioso:
  quem tem escrita pode disparar uma cópia alterada na própria branch. O
  limite real é a lista de colaboradores com escrita; barreira de verdade é
  restringir quem dispara nas configurações do repositório.
- Rascunho não publicado: apague draft e tag se preciso;
  publicado por engano: nunca reutilize a versão, lance uma superior.
  Tags herdadas (`v2.20.0`, `base/codync-*`) não são release da Mexerica.
