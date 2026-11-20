# Heartbeat das Rotinas: checar em silêncio, avisar por exceção

Uma rotina de checagem olha uma fonte de tempos em tempos e só chama o dono quando aparece uma decisão
nova. Sem novidade, não há mensagem. Ela usa o agendador e o histórico das Rotinas que já existem; não há
segundo agendador.

## Como o dono liga

Pelo app, ao salvar a rotina, o dono declara:

| Campo | O que é |
|---|---|
| `fonte` | Nome da fonte no registro do Hub (letras minúsculas, números, ponto, hífen, sublinhado). |
| `escopo` | O trecho da fonte que vale observar. |
| `criterio.estados` | Os estados de um item que pedem decisão. Quando um item cai num deles, o dono é chamado. |
| `destinatario` | Hoje só `dono`. |
| `intervaloMinimoS` | Menor intervalo entre duas consultas à fonte, de 60 s a 7 dias (padrão 300 s). |

Campo desconhecido é recusado: não há onde esconder uma permissão de enviar ou excluir. A checagem nasce
**pausada** e só liga quando o dono salva com `enabled: true`. O gatilho é só de horário (relógio ou
intervalo): webhook, evento e teste manual são recusados. O texto de instrução da rotina é trocado por um texto
fixo, que não pede nenhuma ação.

O modelo (o bot) não declara, não altera (declaração, horário, tempo limite), não apaga nem liga uma checagem; ele só pode pausá-la. Uma rotina comum só vira checagem se não tiver execução em andamento e se não tiver sido criada por um bot. Duplicar uma checagem pelo app mantém a declaração, e a cópia nasce pausada.

## O que a checagem nunca faz

- Não chama a IA.
- Não envia, exclui nem publica nada: a fonte é lida por uma interface sem nenhum método de escrita e a
  checagem não herda permissão do bot.
- Não guarda o conteúdo da fonte: só resumos (hash) do marcador e das pendências.
- O marcador da fonte precisa mudar a cada alteração do escopo; marcador igual dispensa ler o conteúdo. Uma fonte que não garante isso (uma simples contagem, por exemplo) deve devolver «sem marcador», e a checagem compara o conteúdo.

## Os quatro resultados

| Resultado | Quando | O que o dono vê |
|---|---|---|
| Sem mudança | A fonte não mudou desde a última leitura (marcador igual, ou conteúdo igual). Com marcador igual, o conteúdo nem é lido. | Nada. |
| Mudança sem decisão | O conteúdo mudou, mas nenhuma pendência nova pede decisão. | Só o histórico. |
| Decisão pendente | Há pendência nova no critério. Um aviso curto com o fato e a escolha (até 5 pendências por aviso; as que não couberam saem no aviso seguinte). | Um aviso. A mesma pendência em quatro checagens seguidas gera um aviso, não quatro. |
| Fonte indisponível | A fonte não respondeu (sem acesso, fora do ar, limite). | Falha visível, nunca «sem mudança». Avisa na hora se exige ação (sem acesso) ou se o motivo passa a exigir ação; se for passageira, só quando persistir por três checagens. |

Enquanto a fonte falha, a checagem espera cada vez mais entre as tentativas (até 6 horas, ou o intervalo declarado, se for maior) e não apaga o que já
sabia: ao voltar, uma pendência já avisada não é avisada de novo. Uma pendência que sai da fonte e volta é
decisão nova.

## Estado desta entrega

A declaração, a validação, as regras acima e as guardas de permissão estão no Hub; o agendador passa a
consultar a fonte na entrega seguinte. Ainda não existe fonte real: o registro de fontes nasce vazio, e uma
rotina que aponta para uma fonte não registrada fica como fonte indisponível. Testes usam fontes fictícias.
