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
`comentario`) e a linha. Ela nunca repete o trecho achado e lista no máximo 50 achados.

## Onde roda

- **Bastidor** (`.github/workflows/bastidor.yml`): em `pull_request_target` (aberto, editado,
  reaberto, novo push). Um job só, leve. Corrigir a descrição ou o título refaz a checagem sem rodar
  o resto do CI. É o **único** workflow do repositório em `pull_request_target`, e por um motivo:
  ele roda a partir da branch protegida, então o PR não consegue editá-lo nem desligá-lo, e PR de
  fork também é verificado. Para isso ser seguro: permissões só de leitura, nenhum checkout da ref do
  PR, nenhum código do PR é executado, e título, descrição, branch e commits chegam ao verificador
  como dado, lidos pela API, nunca em linha de comando. Se o PR mudar durante a verificação, a
  execução reprova e a mais nova verifica o estado atual.
- **Bastidor em comentários** (`.github/workflows/bastidor-comentarios.yml`): em comentário de PR
  ou issue, comentário de revisão em linha e corpo de revisão. Se o autor tem escrita no
  repositório ou é a automação do projeto, o job lê o comentário de novo e, só se o texto atual
  ainda reprovar, o remove (o corpo de uma revisão enviada não pode ser apagado pela API e fica
  oculto, o que só é anunciado depois de a API confirmar). Um aviso neutro diz a categoria.
  Comentário de gente de fora e de robôs de terceiros não é tocado, e nem liga uma máquina.

Os dois baixam o verificador e as regras da branch padrão pela API, os dois do mesmo commit, e não
executam nada que venha do PR. Se o download falhar, o job falha.

## Regras, segredo e variável

- `.github/security/bastidor.json`: regras genéricas (padrões em minúsculas e sem acento).
- **Segredo** de Actions `BASTIDOR_NOMES_INTERNOS`: o que revelaria nomes internos não fica no
  código nem em variável (variável aparece no log e não é mascarada). É um JSON em uma linha:

```json
{"prefixosTarefa":["AB","CD"],"plataformas":["nome-da-plataforma"],"termosInternos":["termo"]}
```

  - `prefixosTarefa` (2 a 8 letras; reprova `AB-123` e `ab_123`) e `plataformas` são obrigatórios;
    `termosInternos` é opcional. Reprovam a palavra inteira, sem maiúscula e acento.
  - Sem o segredo, vazio ou inválido, a checagem de PR **reprova** e a moderação de comentários
    falha **sem escrever nada**. Nenhum passo imprime a lista; o verificador ainda mascara cada termo.
- **Variável** de Actions `BASTIDOR_AUTOMACAO` (sem sigilo): array JSON com os logins das contas de
  automação do projeto cujo comentário também é removido, por exemplo `["conta-da-automacao[bot]"]`.
  Vazia, nenhum robô é tocado. Não inclua robô cujos comentários devam ser mantidos (revisores
  automáticos, por exemplo).

## Rodar antes de publicar

```sh
# commits e branch atuais contra a main; título e descrição são opcionais
node tools/ci-bastidor.mjs local --base origin/main --titulo "título do PR" --descricao descricao.md
```

Sem `BASTIDOR_NOMES_INTERNOS` no ambiente, o modo local roda **parcial** (só as regras genéricas) e
avisa. Os testes do verificador: `node --test tools/ci-bastidor.test.mjs`.

## Se reprovar

Reescreva o trecho da linha indicada só com o que mudou, por quê, como foi testado e o que ficou
sem provar. Em commit já publicado, reescreva a mensagem e faça o push do branch. Um falso
positivo (por exemplo, frase de produto parecida com um crédito) também se resolve reescrevendo.

## O que a trava não pega

- Frase quebrada em duas linhas, homógrafos de outros alfabetos, entidades HTML além de espaço
  rígido, links encurtados e variações de redação que as regras não conhecem.
- Texto dentro de arquivos do PR (código, docs): quem confere é a revisão e o job de vazamento.
- Texto já publicado: título, descrição ou comentário podem ter sido vistos ou notificados antes da
  remoção e ficam no histórico de edição; a trava reduz a exposição, não a desfaz.
- Citação: se alguém com escrita repetir em comentário próprio um texto de fora, o comentário é
  tratado como dele e removido.
- Issues (só PR e comentários nesta fatia) e o texto de revisão enviada, que só é ocultado.
