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
- Os tetos valem na coleta do turno, na cópia guardada e no processo vivo: o contexto de cada chamada ao provedor, o estado do agente, a lista do turno em andamento (cada mensagem é limitada ao terminar, no próprio objeto que o laço acumula) e a mensagem final.
- Estourou: sai a volta mais antiga inteira. Numa volta só, ficam o pedido e os grupos mais novos que couberem. Uma chamada de ferramenta e os resultados dela saem sempre juntos.

## Pares de chamada e resultado

Um resultado de ferramenta só é guardado se responde a uma chamada ainda aberta da última mensagem do assistente: mesmo id, mesmo nome da ferramenta e um resultado por chamada. Um id repetido na mesma mensagem não é guardado. O receptor do motor aplica a mesma regra, devolve só os campos permitidos e recusa a lista inteira se um item for inválido.

## Identidade e credencial

- A cópia tem como chave o bot e a identidade da conversa (Space, conta e revisão). Invalidar uma sessão apaga só a cópia dela; as outras conversas do mesmo bot ficam.
- A cada turno o Hub lê o cofre uma vez. Dessa leitura saem as chaves que a redação precisa conhecer e a geração da credencial de cada provedor (um resumo em memória, nunca registrado). O cofre que o processo do motor enxerga é preso a essa geração: se a credencial mudar depois da autorização, a leitura é recusada e a chamada ao provedor falha, em vez de a conta nova receber o contexto autorizado para a anterior.
- A decisão de proteção vem antes de qualquer retomada. Se o cofre não pode ser lido, ou uma chave é curta demais para ser achada no texto (menos de 8 caracteres), a cópia é apagada, o processo vivo que ainda guardava o contexto é descartado e o turno não é guardado. Trocar a credencial de um provedor recomeça só as conversas dele.

## Retenção

O arquivo tem teto de 64 conversas e 12 horas sem atividade, contadas da última mensagem, da reabertura ou do fechamento da sessão ou da conexão. Uma tarefa de limpeza apaga as vencidas a cada 10 minutos; uma conversa de sessão ainda aberta não é removida, e com todas as vagas em uso a conversa nova é recusada. Não é um descarte pontual em 12 horas: a cópia pode ficar na memória até a próxima rodada da limpeza.

## Windows

O módulo `historico` só existe em Unix, e o Hub no Windows o chama só por `motor_mexerica::forget_conversation`, que lá não faz nada. A evidência de build é `cargo check --target x86_64-pc-windows-gnu`, feito com o compilador C do Zig como substituto. Isso prova que as referências compilam, mas não prova link, build MSVC, instalador nem execução no Windows.

## Testes

- `motor/`: `npm test`. Os testes de `motor/mexerica/test/history.test.ts` cobrem os tetos no processo vivo, o receptor, os pares e o acumulador do turno.
- `host/`: `fork::motor_mexerica` (cópia enxuta, redação, tetos, retenção, credencial presa à geração, cofre indisponível, troca de provedor, queda e cancelamento) e `agent::bot::motor_conversa_tests` (troca de modelo e de conta pelo ator real do bot). Rode com `MEXERICA_MOTOR_TESTS_REQUIRED=1` para que a falta do Node reprove em vez de pular.
