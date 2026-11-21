# Agenda interna dos bots

A agenda da Mexerica é o trabalho dos próprios bots em
[Rotinas](routines.md). Todos os bots, inclusive o Berga, podem criar tarefas
recorrentes e recordações pontuais para si. Não há cartão Agenda em
Configurações → Plugins, interruptor de plugin ou autorização por bot.

O bot usa o MCP `routines`: `save_routine` com `cron` para repetição ou `once`
para uma recordação com data e horário. As tarefas aparecem em Rotinas e cada
Space mostra somente as rotinas dos seus bots. IDs de outro bot ou Space não
autorizam leitura, alteração, disparo ou exclusão por essas ferramentas.

A disponibilidade padrão mantém as proteções de Rotinas: mudanças dependem de
pedido do dono na sessão de origem, vínculo válido, pausa global e limites de
execução. Um turno de rotina, webhook, outro bot ou aviso do Hub não autoriza
criar novas rotinas. A criação autônoma sem pedido continua fora deste recurso.

## Dados anteriores

A retirada do plugin não apaga nem converte eventos, calendários, instalações,
recursos ou concessões antigos. As tabelas e migrações necessárias para abrir
bancos existentes permanecem. Os interruptores antigos deixam de controlar o
agendamento; as RPCs `donnaAgenda`, `donnaAgendaSave` e `donnaAgendaRead` saem.
Um app antigo precisa ser atualizado para deixar de pedir essas RPCs.

## Agendas externas

Google, Apple e outras agendas do dono não são integradas por este recurso.
Scatty é um produto separado, acessível por conectores MCP conforme sua própria
configuração. Não há importação automática nem novo agendador.

