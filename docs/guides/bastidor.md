# Trava contra bastidor

Este repositório é público: título e descrição de PR, nome de branch, mensagem de commit e
comentário só podem dizer **o que mudou, por quê, como foi testado e o que ficou sem provar**.
Nada de bastidor de quem escreveu ou coordenou o trabalho. Uma trava automática reprova o PR
que trouxer esse tipo de texto.

## O que a trava procura

| Categoria | Exemplos do que reprova |
|---|---|
| `plataforma` | link, selo ou id de conversa de plataforma de agentes; marcador `...-badge:start`; nome da plataforma interna |
| `tarefa` | código de tarefa interno em branch, título, descrição ou commit; «Closes XX-123» |
| `autoria-ia` | crédito de autoria ou revisão por modelo ou ferramenta de IA («feito com…», «revisado por…», trailer de coautor de robô, branch com prefixo de ferramenta) |
| `coordenacao` | quem decidiu ou coordena, relatório interno, subagente, custo da rodada, contagem de máquinas |

Nome de produto que é o **assunto** da mudança passa: «conectar o Codex», «motor Cline»,
«Claude pelo programa oficial». O que reprova é o crédito: autoria («feito com», «escrito por»)
ou revisão («revisado por») atribuída a um modelo ou ferramenta de IA.

A saída informa a categoria, o campo (`titulo`, `descricao`, `branch`, `commit abc1234`,
`comentario`) e a linha. Ela nunca repete o trecho achado.

## Onde roda

- **Bastidor** (`.github/workflows/bastidor.yml`): em `pull_request` (aberto, editado, reaberto,
  novo push). Um job só, leve. Corrigir a descrição ou o título refaz a checagem sem rodar o resto
  do CI. Título, descrição e commits são lidos pela API a cada execução.
- **Bastidor em comentários** (`.github/workflows/bastidor-comentarios.yml`): em comentário de PR
  ou issue, comentário de revisão em linha e corpo de revisão. Se o autor tem escrita no
  repositório ou é a automação do projeto, o comentário é removido (o corpo de uma revisão enviada
  não pode ser apagado pela API e fica oculto) e um aviso neutro diz a categoria. Comentário de
  gente de fora e de robôs de terceiros não é tocado, e nem liga uma máquina.

Os dois baixam o verificador e as regras da branch padrão pela API e não executam nada que
venha do PR. Só o PR que introduz o verificador usa a versão do próprio PR.

## Regras e variável do repositório

- `.github/security/bastidor.json`: regras genéricas (padrões em minúsculas e sem acento).
- Variável de Actions `BASTIDOR_NOMES_INTERNOS`: o que revelaria nomes internos não fica no código.
  É um JSON em uma linha:

```json
{"prefixosTarefa":["AB","CD"],"plataformas":["nome-da-plataforma"],"termosInternos":["termo"],"automacao":["conta-da-automacao[bot]"]}
```

  - `prefixosTarefa`: prefixos de código de tarefa (2 a 8 letras); reprova `AB-123` e `ab_123`.
  - `plataformas` e `termosInternos`: palavras que não podem aparecer; reprova a palavra inteira.
  - `automacao`: contas de robô do próprio projeto cujo comentário também é removido.
  - Todas as chaves são opcionais. Valor inválido faz a checagem falhar sem mostrar o conteúdo.
  - Sem a variável, o verificador roda só com as regras genéricas e emite um aviso.

## Rodar antes de publicar

```sh
# commits e branch atuais contra a main; título e descrição são opcionais
node tools/ci-bastidor.mjs local --base origin/main --titulo "título do PR" --descricao descricao.md
```

Para incluir os nomes internos, exporte `BASTIDOR_NOMES_INTERNOS` com o mesmo valor da variável.
Os testes do verificador: `node --test tools/ci-bastidor.test.mjs`.

## Se reprovar

Reescreva o trecho da linha indicada só com o que mudou, por quê, como foi testado e o que ficou
sem provar. Em commit já publicado, reescreva a mensagem e faça o push do branch. Um falso
positivo (por exemplo, frase de produto parecida com um crédito) também se resolve reescrevendo.

## O que a trava não pega

- Frase quebrada em duas linhas e variações de redação que as regras não conhecem.
- Texto dentro de arquivos do PR (código, docs): quem confere é a revisão e o job de vazamento.
- Título, descrição ou comentário depois de publicados e lidos: o texto pode ter sido visto ou
  notificado antes da remoção; a trava reduz a exposição, não a desfaz.
- Issues e o corpo de comentários de fork; texto de revisão enviada só é ocultado.
