# Motor Mexerica: conversa guardada pelo Hub

O Motor Mexerica é um processo Node separado (`motor/mexerica`) que roda o laço de agente do Pi e fala com o Hub por JSONL em stdin/stdout. Ele é experimental e fica desligado: só aparece no registro de motores quando o Hub sobe com `MEXERICA_MOTOR=1`.

Este documento descreve só como a conversa sobrevive ao processo. O processo do motor é descartável: cancelar o turno, trocar de provedor ou de modelo no app, ou uma queda o encerram. Quem guarda a conversa é o Hub (`host/src/fork/motor_mexerica/historico.rs`), e o processo novo a recebe de volta antes do próximo turno (comando `restore_history`).

## O que é guardado

- Só texto de usuário, texto de assistente, resultados de ferramenta e as chamadas de ferramenta (id, nome, argumentos), mais o mínimo que o provedor precisa para receber a conversa de novo. Pensamento, detalhes, erros, imagens e mensagens de assistente com erro ou abortadas ficam de fora.
- Tudo passa pela redação do Guard (`guard_hub::mcp::redigir_json`) antes de ser guardado: chaves do cofre, formatos comuns de credencial, cookies e campos de credencial pelo nome, inclusive nos argumentos das ferramentas. Os rótulos (`api`, `provider`, `model`), o uso e o custo entram só com campos conhecidos; um rótulo ou id que a redação alteraria não é guardado. A redação é de melhor esforço e não promete achar todo segredo.
- A cópia vive só na memória do Hub, nunca em disco nem em log. Se o Hub reinicia, a conversa recomeça.

## Uma medida só para os tetos

Todos os tetos usam a mesma unidade: bytes UTF-8 do JSON na forma em que ele cruza a fronteira entre o Hub e o motor.

- 256 KiB por mensagem, 2 MiB para o array inteiro (colchetes e vírgulas incluídos) e 2000 mensagens por conversa.
- O Hub manda cada mensagem como um texto JSON e o receptor do motor mede esses mesmos bytes, sem serializar de novo (os serializadores do Rust e do Node imprimem alguns números de modos diferentes). Um teste fecha o array exatamente no teto, com um número que cada serializador imprime com tamanho diferente, e confere que o receptor real aceita esse histórico e recusa um item a mais.
- Os tetos valem na coleta do turno, na cópia guardada e no processo vivo: o contexto de cada chamada ao provedor, o estado do agente, a lista do turno em andamento (cada mensagem é limitada ao terminar, no próprio objeto que o laço acumula, e o evento de fim de volta vai sem a carga) e a mensagem final. Os argumentos de uma chamada de ferramenta nunca mudam antes de ela ser executada: o corte da mensagem do assistente que tem chamadas vem no fim da volta. Enquanto uma volta de ferramentas está aberta (há chamada sem resultado), o corte do estado vivo espera ela fechar, para não deixar resultados sem a chamada.
- Estourou: sai a volta mais antiga inteira. Numa volta só, ficam o pedido e os grupos mais novos que couberem. Uma chamada de ferramenta e os resultados dela saem sempre juntos.

## Pares de chamada e resultado

Um resultado de ferramenta só é guardado se responde a uma chamada ainda aberta da última mensagem do assistente: mesmo id, mesmo nome da ferramenta e um resultado por chamada. Um id repetido na mesma mensagem não é guardado. Uma volta que não terminou (turno cancelado ou processo morto no meio) ganha, para cada chamada sem resultado, um resultado de erro "resultado desconhecido: a ação pode ter sido executada; confirme antes de repetir": a conversa guardada nunca tem chamada sem resultado, e o modelo é avisado de que a ação pode ter sido executada, mas isso não impede que ele a peça de novo (nova decisão de permissão no cartão). O receptor do motor exige o mesmo (cada chamada com o resultado dela antes da próxima mensagem), devolve só os campos permitidos e recusa a lista inteira se um item for inválido.

## Identidade e credencial

- A cópia tem como chave o bot e a identidade da conversa (Space, conta e revisão). Invalidar uma sessão apaga só a cópia dela; as outras conversas do mesmo bot ficam.
- A cada turno o Hub lê o cofre uma vez. Dessa leitura saem as chaves que a redação precisa conhecer e a geração da credencial de cada provedor (um resumo em memória, nunca registrado). O cofre que o processo do motor enxerga é preso a essa geração: se a credencial mudar depois da autorização, a leitura é recusada e a chamada ao provedor falha, em vez de a conta nova receber o contexto autorizado para a anterior.
- A decisão de proteção vem antes de qualquer retomada. Se o cofre não pode ser lido, ou uma chave é curta demais para ser achada no texto (menos de 8 caracteres), a cópia é apagada, o processo vivo que ainda guardava o contexto é descartado e o turno não é guardado. O processo que viu esse turno sem proteção também não volta a ser usado, mesmo que o cofre se recupere: o turno seguinte começa de uma conversa que passou pela redação. Um segundo prompt na mesma sessão, enquanto há um turno em curso, é recusado antes de tocar na conversa. Trocar a credencial de um provedor recomeça só as conversas dele.

## Retenção

O arquivo tem teto de 64 conversas e 12 horas sem atividade, contadas da última mensagem, da reabertura ou do fechamento da sessão ou da conexão. Uma tarefa de limpeza apaga as vencidas a cada 10 minutos; uma conversa de sessão ainda aberta não é removida, e com todas as vagas em uso a conversa nova é recusada. Não é um descarte pontual em 12 horas: a cópia pode ficar na memória até a próxima rodada da limpeza.

## Reabrir a sessão

Trocar de modelo no app fecha a conexão e o bot abre outra com `session/load`. O motor retoma sem modelo escolhido e não anuncia nenhum, então o bot seleciona o modelo configurado de novo depois do `load`, como faz na sessão nova.

## Nova tentativa e cache do pedido

- Uma recusa do fornecedor **antes** de qualquer processamento ou stream (408, 409, 429 e 5xx) é repetida no mesmo pedido e na mesma conta: no máximo duas repetições por chamada, cada espera dentro do que o fornecedor pediu, até 30 s, e no máximo 60 s de espera de repetição por turno, somando todas as chamadas dele (uma volta de ferramentas gasta o mesmo orçamento). Espera pedida acima do teto falha na hora, com a mensagem do teto; orçamento do turno gasto encerra a repetição. Cancelar o turno durante a espera encerra o pedido sem chamada nova.
- Erro de transporte sem status HTTP (a conexão caiu) não é repetido: o fornecedor pode ter recebido e processado o pedido, e repetir ali duplicaria geração e cobrança.
- 400 (inclusive estouro de contexto), 401, 403, 404, 413 e 422 nunca são repetidos, mesmo que o fornecedor peça no cabeçalho `x-should-retry`.
- A repetição acontece na abertura da resposta HTTP: resposta que já começou a chegar, geração incerta e ferramenta já executada pelo Hub não são repetidas, e a conta (nem o plano) nunca é trocada por causa de uma recusa. No Google a repetição também vale, mas sem o `retry-after`: o SDK dele não expõe os cabeçalhos, então a espera é a do próprio motor.
- A identidade de cache é um valor aleatório do Hub por conversa, conta e fornecedor — nunca o id da sessão que a ponte usa para controle, o id do bot ou texto da pessoa. O motor a manda como identidade de sessão do pedido (a chave de cache da OpenAI e os cabeçalhos de afinidade de quem os usa); sem ela, o campo não existe no pedido; trocar de conta ou de fornecedor cria outro valor.
- `set_system_prompt` substitui o prompt anterior: a transcrição passa a ter uma única mensagem de sistema, no início, em vez de somar o texto de duas.

## Limites conhecidos

- A conversa guardada não garante que uma ação externa aconteça uma única vez. Se o serviço aceitou a ação mas o motor não recebeu o resultado (queda, cancelamento, teto de tamanho), o modelo vê "resultado desconhecido" e uma nova geração ainda pode pedir a mesma ação, com nova decisão de permissão no cartão. Para ações que não podem se repetir é preciso conferir o estado do serviço ou ter idempotência; restaurar a conversa nunca reexecuta uma ferramenta.

- Em bots de Space, trocar só o modelo abre uma sessão nova (o carimbo de execução de Space inclui o modelo): a conversa recomeça pelo resumo que o bot já monta, não pela cópia guardada. É a regra de todos os motores.
- Uma volta de ferramentas cujos resultados juntos passam do teto de 2 MiB sai inteira do contexto do provedor; o modelo pode repetir as chamadas. O teto é rígido de propósito.
- Credencial composta só de dígitos e guardada como número JSON (não como texto) não passa pela redação, que atua sobre texto e nomes de campo; a redação é de melhor esforço.
- Reabrir ou fechar uma sessão enquanto um turno dela espera uma decisão de ferramenta não é coberto: o bot fecha a conexão depois do turno.

## Windows

O módulo `historico` só existe em Unix, e o Hub no Windows o chama só por `motor_mexerica::forget_conversation`, que lá não faz nada. A evidência de build é `cargo check --target x86_64-pc-windows-gnu`, feito com o compilador C do Zig como substituto. Isso prova que as referências compilam, mas não prova link, build MSVC, instalador nem execução no Windows.

## Testes

- `motor/`: `npm test`. Os testes de `motor/mexerica/test/history.test.ts` cobrem os tetos no processo vivo, o receptor, os pares e o acumulador do turno; `retentativa.test.ts` cobre a nova tentativa (limites, veto, cancelamento e ferramenta não repetida) e `cache.test.ts` a identidade de cache e o prompt de sistema que substitui.
- `host/`: `fork::motor_mexerica` (cópia enxuta, redação, tetos, retenção, credencial presa à geração, cofre indisponível, troca de provedor, queda e cancelamento) e `agent::bot::motor_conversa_tests` (troca de modelo e de conta pelo ator real do bot). Rode com `MEXERICA_MOTOR_TESTS_REQUIRED=1` para que a falta do Node reprove em vez de pular.
