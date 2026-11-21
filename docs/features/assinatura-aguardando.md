# Assinatura no limite: Aguardando

Em **chat direto iniciado pelo usuário** e nas **execuções de rotina**, quando
Codex ou Claude informa que a assinatura atingiu o limite, o bot fica
**Aguardando**. O pedido e a sessão ficam no SQLite, com o horário previsto
quando disponível. O Mac e a TUI mostram a espera. O host nunca escolhe outro
modelo ou uma API paga automaticamente.

## Rotinas em Aguardando

Uma rotina que atinge o limite **espera e continua**, em vez de falhar. A
execução em curso guarda no mesmo checkpoint da conversa: persiste e volta após
reinício do Hub, retoma quando o limite volta (horário informado ou os dados de
uso já coletados) e respeita o mesmo intervalo mínimo, backoff e prazo máximo de
tentativas. O aviso da espera diz “Rotina «nome» aguardando o limite da
assinatura; volto às HH:MM” e aparece no chat do bot, como nas conversas.

Garantias da espera da rotina:

- A mesma execução **não roda duas vezes**: o gatilho normal não reenfileira uma
  execução que já está em Aguardando, e a retomada prepara uma **continuação**
  da permissão durável original (recibo de cadeia), sem abrir uma execução nova
  nem repetir um `started` já despachado. Durante a espera a execução aparece
  como `recovering`, a mesma forma que assume na recuperação pós-reinício.
- **Desativar, excluir ou cancelar** a rotina ou a execução encerra a espera —
  a execução sai como parada e a vaga do bot volta. A exceção é o disparo de
  teste, que segue o mesmo critério dos gatilhos manuais e roda mesmo desativado.
- O prazo de execução da rotina **não corre durante a espera**: a tentativa
  retomada recebe o orçamento inteiro dela.
- A retomada **passa de novo pelo teto de gasto**: bloqueada, a execução volta a
  Aguardando parada pelo teto e só continua quando ele liberar.
- **Pausa encerra a execução.** Na conversa direta, uma espera pausada (teto de
  tentativas, identidade ou roteamento não comprovados, retomada automática
  desligada, handoff impossível) fica guardada até o dono agir. A rotina é
  trabalho autônomo: uma espera pausada não teria quem destravasse e seguraria
  a vaga do bot para sempre — então a pausa encerra a execução como falha, com
  o aviso no chat. Enquanto a espera está **ativa** (Aguardando/tentando), ela
  segura a vaga do bot — outras rotinas e turnos do mesmo bot
  aguardam; esse é o preço de garantir a execução única.
- O aviso carrega `routineWait: {routineId, runId}` além de `subscriptionWait`.
  O campo `routineId` **não** vai no topo do aviso: o desktop o roteiria para o
  botão "abrir a rotina" e esconderia os controles e o texto da espera.
- Se o limite aparecer **antes da permissão** (por exemplo, na abertura da
  sessão, sem identidade durável de execução), a execução falha como antes —
  estacionar sem identidade deixaria a rotina parada para sempre.

## Ajustes do Hub e desistência por conversa

Em **Ajustes → Conversa → Limites de uso**, os ajustes valem para todas as conversas
no Hub selecionado. São independentes e ficam gravados no Hub:

- **Retomar depois do limite** vem ligado. Desligado, o pedido
  fica guardado e a retomada exige **Retomar agora**. Desligar durante o preparo
  também bloqueia o envio que ainda não recebeu autorização durável.
- **Soneca até o limite voltar** vem desligada. Ligada, com horário informado,
  não há tentativa antes desse horário nem alerta durante a espera. O intervalo
  mínimo e o prazo já agendado continuam valendo. Sem horário, conserva o backoff
  atual. O horário é uma previsão da conta, não uma prova de que já liberou.
- Soneca com retomada ligada espera o prazo e volta sozinha; com retomada
  desligada, atualiza o aviso quando o prazo chega, sem enviar ao motor.

O aviso de cada espera oferece **Não retomar**. A confirmação só vem depois de
salvar uma barreira durável com o ID da espera, do aviso e da conversa/sessão.
Ela impede a retomada daquela espera mesmo após reinício ou mudança dos ajustes.
O cancelamento de uma espera não cancela mensagens na fila nem outras conversas:
o pedido e a sessão são guardados à parte e o ator libera a fila.

Antes do envio, o Hub confere a barreira novamente na transação da permissão,
depois de todo preparo. Se a permissão já tinha sido concedida, o Hub informa que
o pedido pode ter iniciado, solicita a parada existente somente para esse turno
e não oferece replay do pedido incerto. Resultado já concluído continua somente
para publicação; nunca é descartado para chamar o motor outra vez.

**Retomar agora**, numa espera cancelada antes da permissão, é uma intenção nova
explícita: exige o bot livre, conserva o pedido, a sessão, a seleção autorizada,
a política e a identidade original, e passa pelas revalidações existentes. A
espera ganha outro ID: cliques no aviso antigo não a cancelam nem a retomam.
Uma soneca com horário conhecido continua respeitando o prazo na ação manual.

Os RPCs de dono são `donnaUsageLimits`, `donnaUsageLimitsSave`,
`donnaUsageWaitCancel` e `donnaUsageWaitResume`; não são ferramentas do bot.
Os estados `subscriptionWait` do aviso são espelhados com `MIRROR_SCHEMA` novo.

## Tentativas e cancelamento

O host retoma na sessão salva após o reset, respeitando um intervalo mínimo de
cinco minutos entre tentativas. Sem reset, tenta após 5, 10, 20, 40 e 60 minutos.
Depois de cinco retomadas bloqueadas ou interrompidas por crash, **com ou sem reset informado**, fica
**Pausado** e oferece **Retomar agora** no app atualizado; clientes anteriores e a TUI mantêm
**Tentar de novo** e **Parar** (`r` e `s`).
Cada retomada conta e salva o próximo prazo antes do envio. O contador e a pausa
sobrevivem a reinícios. Tentar de novo ou trocar explicitamente o modelo inicia
um novo orçamento de tentativas.

Mensagens enviadas durante a espera ficam na fila durável, em ordem, com anexos e
citações. O pedido atual retoma primeiro; as demais mensagens formam o próximo
turno normal. A marca de recuperação da fila é removida quando a entrada passa a
`sent`; ela não cria uma nova espera de assinatura. Apenas **Parar pelo usuário**
cancela também as mensagens enfileiradas até esse momento; mensagens enviadas
depois seguem normalmente. Cancelamento do turno pelo host preserva a fila.
Comandos atrasados consultam os IDs e o status persistido para impedir reenvios.

Trocar o comando ACP nas configurações (ou o motor de uma conversa sem seleção
própria legível) durante a espera abre uma nova sessão com handoff (resumo
disponível, pedidos já enviados, última resposta e plano). A troca de modelo da
conversa não muda o pedido em espera: ele retoma com a seleção admitida,
e a troca vale no próximo turno. Mensagens ainda
na fila não entram nele. O handoff é consumido depois de enviar o prompt: as
retomadas da mesma sessão não o acrescentam de novo. Se a recarga falhar e for
necessária outra sessão, o host reconstrói o contexto antes de enviar o pedido.

O checkpoint de conclusão guarda a resposta completa antes de publicar o resultado.
Se o host cair durante a publicação, ele a termina localmente, sem consultar o
modelo outra vez. Falhas de SQLite são repetidas antes de enviar novos prompts.
A recuperação da espera ativa não depende da janela de uma hora do resume comum.

## Turnos que encerram ao atingir o limite

**Compactação, turnos ocultos ou retomados pelo resume de crash comum, pedidos
de outros bots e turnos de canal não entram em Aguardando.** Eles encerram com
aviso claro e liberam a fila. A compactação não é reenviada como mensagem de
conversa. Checkpoints antigos de espera desses caminhos são encerrados
localmente, sem novo prompt. Trabalho autônomo de fundo que não seja execução
de rotina também continua fora da espera nesta etapa.

## Detecção e limites da garantia

- Só resultados do adapter que provam o limite estacionam o chat. Codex usa
  `codex_error_info`, códigos equivalentes ou mensagens inteiras conhecidas;
  Claude usa `errorKind=rate_limit` e equivalentes.
- Um evento `usage_update._meta["_claude/rateLimit"]` com `rejected` só completa
  o reset de um resultado de limite. Ele não transforma um erro comum em bloqueio.
  O hint expira em um minuto; `allowed` e `allowed_warning` o limpam imediatamente.
  `end_turn` concluído nunca é reenviado por causa desse evento.
- HTTP 429 genérico, indisponibilidade, autenticação, contexto cheio, stderr e
  erros de ferramentas/MCPs não bastam para classificar limite de assinatura.
- Ao estacionar, o host encerra ferramentas pendentes e expira permissões. Uma
  falha ao preparar o handoff pausa a retomada até a ação do usuário.
- O uso local já coletado pode completar o reset, sem consulta adicional de IA.
- ACP não garante execução exatamente uma vez de ferramentas. A continuação
  preserva o contexto e orienta a não repetir ações concluídas; um crash entre
  envio e checkpoint ainda tem a ambiguidade inerente ao protocolo.
