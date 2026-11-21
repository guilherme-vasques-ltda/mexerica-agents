# Excluir uma sessão extra

Na lista lateral de Sessões, abra o menu da extra e escolha **Excluir**. O Host
precisa anunciar essa capacidade. Hubs antigos mantêm a opção indisponível. A
aba do topo mantém cópia e arquivamento, sem exclusão. A conversa principal é
protegida, pois seu ID também identifica a conversa do bot.

A confirmação mostra nome e ID e informa a exclusão definitiva da sessão e
das mensagens desta sessão. Anexos em disco, cópias externas do motor, resumos
já levados a outras sessões, memória do bot e metadados com o ID da sessão
podem permanecer. Outras sessões e rotinas não são alteradas. Cancelar recebe
o foco; Escape cancela. Os seis idiomas do app, incluindo árabe e RTL, são atendidos.
Botão direito, Shift+F10 ou tecla de menu abrem as mesmas ações da lista.

A ação pertence apenas ao dono, com a autorização original reconferida na
transação. Hub, Space, bot, sessão e revisão são obrigatórios. Um bot/MCP não
pode usar esse método. A sessão ocupada (turno, admissão, conclusão, espera ou
trabalho pendente) não é apagada: o app orienta encerrar antes. Outra sessão do
mesmo bot pode continuar trabalhando.

Sessão, mensagens e registros de escolhas dessa sessão são removidos juntos.
A falha de qualquer escrita desfaz tudo. Não se alteram memória do bot, suas
marcas de procedência, outras sessões ou rotinas. A ação não garante apagar
anexos em disco, cópias externas do motor nem cartões ou resumos de continuação
já levados a sessões derivadas. Metadados com o ID da sessão podem permanecer,
inclusive o registro de remoção descrito abaixo.

Repetir o mesmo pedido devolve a mesma confirmação, sem apagar mais nada.
Um registro de remoção guarda só os IDs e revisões necessários para repetição e
sincronização, sem nome nem mensagens. Ele é reenviado na reconexão, mesmo com
cursor avançado, para limpar caches de apps antigos. Entradas atrasadas da
sessão apagada são recusadas no banco e no mirror do app; a aba apagada volta à
principal. O projeto não ganhou um campo persistido novo no cache do desktop.
