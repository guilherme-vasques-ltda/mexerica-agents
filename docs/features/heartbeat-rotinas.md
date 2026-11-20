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

O modelo (o bot) não declara, não altera (declaração, nome, horário, tempo limite, chave de webhook), não apaga nem liga uma checagem; ele só pode pausá-la. Uma rotina comum só vira checagem se não tiver execução em andamento e se não for uma rotina descendente de bot (com agenda e orçamento próprios). Duplicar uma checagem pelo app mantém a declaração, e a cópia nasce pausada.

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

## Como roda

No horário da rotina (relógio ou intervalo), o agendador das Rotinas consulta a fonte sem passar pela IA: uma
rodada por vez por rotina, e nunca antes do intervalo mínimo declarado. O Hub grava, na própria rotina, a última
checagem, o resumo do marcador e as pendências já avisadas; por isso o aviso não se repete depois de reiniciar. Só o
resultado que importa vira execução no histórico (mudança sem decisão, decisão pendente e a primeira falha de uma
sequência); `sem mudança` só atualiza a hora da última checagem, que aparece em `heartbeatStatus` na rotina. O texto
do aviso (fato e escolha) fica só num campo privado até ser publicado (nenhuma listagem o devolve), vai ao dono pelo
chat e pelo alerta do celular e é apagado depois de publicado; o histórico guarda só o resultado.

Só gatilhos de relógio (cron ou intervalo) valem para uma checagem; gatilho único é recusado. O alerta no celular é
de melhor esforço: o aviso no chat é a confirmação durável. As fontes precisam limitar o próprio tempo de leitura e
não entrar em pânico com conteúdo; uma fonte lenta atrasa as demais checagens do mesmo ciclo.

Se o Hub cair justamente durante uma consulta, aquela ocorrência não é refeita: a checagem segue no horário seguinte
da rotina (é uma amostragem periódica, não um compromisso a cumprir). Outra concessão da mesma fonte (outra conta, ou a
fonte registrada de novo) começa do zero: não herda marcador, espera nem pendências já avisadas.

Pausar a rotina, pausar todas as rotinas, apagar a rotina, retirar a concessão da fonte ou editar a declaração no meio
de uma leitura descarta o resultado: nada é gravado nem avisado. Uma fonte só é lida pelo bot a que foi concedida.

## Estado desta entrega

Ainda não existe fonte real: o registro de fontes nasce vazio e uma rotina que aponta para uma fonte não concedida
ao bot fica como fonte indisponível (sem acesso), com alerta. Os testes usam fontes fictícias.
