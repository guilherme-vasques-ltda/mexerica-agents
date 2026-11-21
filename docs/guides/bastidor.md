# Trava contra bastidor

Este repositório é público: título e descrição de PR, nome de branch e mensagem de commit só podem
dizer **o que mudou, por quê, como foi testado e o que ficou sem provar**. Nada de bastidor de quem
escreveu ou coordenou o trabalho. Uma trava automática reprova o PR que trouxer esse tipo de texto.

## O que a trava procura

| Categoria | Exemplos do que reprova |
|---|---|
| `plataforma` | link, selo ou id de conversa de plataforma de agentes; marcador `...-badge:start` |
| `tarefa` | «Closes XX-123» com código de tarefa |
| `autoria-ia` | crédito de autoria ou revisão por modelo ou ferramenta de IA («feito com…», «revisado por…», «gerado por…», trailer de coautor de robô, branch com prefixo de ferramenta) |
| `coordenacao` | quem decidiu ou coordena, relatório interno, subagente, custo da rodada, contagem de máquinas |

Nome de produto que é o **assunto** da mudança passa: «conectar o Codex», «motor Cline»,
«Claude pelo programa oficial». O que reprova é o crédito: autoria («feito com», «escrito por»)
ou revisão («revisado por») atribuída a um modelo ou ferramenta de IA.

A saída informa o campo (`titulo`, `descricao`, `branch`, `commit abc1234`) e a linha, e a
categoria das regras genéricas. O que casa com a **lista interna** aparece só como «texto interno
encontrado», sem dizer qual termo. A saída nunca repete o trecho achado e lista no máximo 50 achados.

## Onde roda

Workflow `.github/workflows/bastidor.yml`, em `pull_request_target` (aberto, editado, reaberto, novo
push). Um job só, leve. Corrigir a descrição ou o título refaz a checagem sem rodar o resto do CI.
É o **único** workflow do repositório em `pull_request_target`, e por um motivo: ele roda a partir da
branch protegida, então o PR não consegue editá-lo nem desligá-lo, e PR de fork também é verificado.
Para isso ser seguro: permissões só de leitura, nenhum checkout da ref do PR, nenhum código do PR é
executado, e título, descrição, branch e commits chegam ao verificador como dado, lidos pela API,
nunca em linha de comando.

- O verificador e as regras vêm da branch padrão pela API, os dois do mesmo commit; se o download
  falhar, o job falha.
- O SHA do PR é fixado no início (o do evento). Título, descrição, branch, SHA e número de commits
  são relidos no fim; se algo mudou, a execução reprova e a rodada do evento seguinte decide.
- **PR de fork** roda só com as regras genéricas e **não recebe o segredo**: o resultado público não
  pode servir de oráculo para a lista interna. A lista só se aplica a PR de branch do próprio
  repositório.
- A concorrência é por número de PR, com cancelamento da rodada anterior.
- **Máquina**: o job usa `vars.CI_MAQUINA_LEVE` (padrão: a máquina leve do CI). Como o job aceita
  eventos de fork, enquanto a variável apontar para uma máquina paga o repositório não pode estar
  público: qualquer pessoa poderia ligar máquinas pagas abrindo PRs. Antes de abrir, aponte a
  variável para a máquina padrão do GitHub.
- Política do GitHub: confirme, antes de abrir, a política efetiva de `pull_request_target` do
  repositório (há uma política padrão que a restringe em repositórios públicos) e que o check
  `bastidor` está associado ao head e exigido no ruleset.

## Regras e segredo

- `.github/security/bastidor.json`: regras genéricas (padrões em minúsculas e sem acento).
- **Segredo** de Actions `BASTIDOR_NOMES_INTERNOS`: o que revelaria nomes internos não fica no
  código nem em variável (variável aparece no log e não é mascarada). É um JSON em uma linha:

```json
{"prefixosTarefa":["AB","CD"],"plataformas":["nome-da-plataforma"],"termosInternos":["termo"]}
```

  - `prefixosTarefa` (2 a 8 letras; reprova `AB-123` e `ab_123`) e `plataformas` são obrigatórios;
    `termosInternos` é opcional. Reprovam a palavra inteira, sem maiúscula e acento.
  - Em PR do próprio repositório, sem o segredo, vazio ou inválido, a checagem **reprova**.
  - Nenhum passo imprime a lista; o verificador ainda mascara cada termo. A máscara é uma defesa
    auxiliar: termos curtos mascarados podem esconder pedaços de texto comum no log.

## Rodar antes de publicar

```sh
# commits e branch atuais contra a main; título e descrição são opcionais
node tools/ci-bastidor.mjs local --base origin/main --titulo "título do PR" --descricao descricao.md
```

Sem `BASTIDOR_NOMES_INTERNOS` no ambiente, o modo local roda **parcial** (só as regras genéricas) e
avisa. Os testes do verificador: `node --test tools/ci-bastidor.test.mjs`.

## Se reprovar

Reescreva o trecho da linha indicada só com o que mudou, por quê, como foi testado e o que ficou
sem provar. Em commit já publicado, reescreva a mensagem e atualize o branch com
`git push --force-with-lease origin <branch-do-pr>` (confira o branch antes; a opção recusa a
atualização se o remoto mudou). Um falso positivo (por exemplo, frase de produto parecida com um
crédito) também se resolve reescrevendo.

## O que a trava não pega

- Frase quebrada em duas linhas, homógrafos de outros alfabetos, entidades HTML além de espaço
  rígido, links encurtados e variações de redação que as regras não conhecem.
- Texto dentro de arquivos do PR (código, docs): quem confere é a revisão e o job de vazamento.
- Texto já publicado: título e descrição podem ter sido vistos ou notificados antes da checagem e
  ficam no histórico de edição; a trava reprova o PR, não desfaz a publicação.
- Comentários, revisões e issues: não são verificados nesta fatia.
- Limites: 2 milhões de caracteres somados por PR (acima disso a checagem falha em vez de truncar),
  200 achados e 50 anotações.
