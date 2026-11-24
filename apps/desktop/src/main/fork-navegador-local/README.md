# Guardas do navegador local

O módulo não é carregado pelo Desktop. A flag estática fica desligada e não existe adaptador de
navegação real. O ciclo e a superfície deste recorte são exercitados exclusivamente por fixtures
em memória. Não há janela de navegador, entrada de pessoa ou bot, integração com o Hub, IPC ou rede.

O contrato do gate é para um futuro adaptador confiável do Hub local no processo principal. Nenhum
campo do pedido é prova de origem ou autorização. A autenticação e a decisão por ação, workspace,
bot, geração e instância ficam nesse adaptador, que não existe neste recorte. Os eventos do ciclo
são para o dono do processo, separados dos comandos; não devem ser expostos ao renderer ou à página.
A perda do Hub, a troca de geração e a destruição do app aposentam o ciclo inteiro.

A superfície só oferece captura. O limite de bytes vale para o que o ciclo devolve, não para a
memória usada ao capturar. Os bytes são conteúdo não confiável; nada os decodifica ou executa aqui.
Ocultar a captura é responsabilidade do consumidor futuro e não altera a capacidade de parar ou
revogar. Após invalidar, o ciclo tenta fechar e limpar armazenamento e cache. Sem confirmação dos
três passos, não libera a vaga nem reutiliza a partição. O adaptador futuro terá de confirmar que
não há mais trabalho da superfície que possa voltar a escrever nela antes de declarar a limpeza.

A política de URL só classifica endereços de fixtures `.invalid`, sem resolver DNS nem abrir
conexões. Ela não prova o IP efetivo nem contém WebRTC ou outros caminhos de rede. O adaptador
real precisa de contenção independente de rede e de processo, revisão de segurança e prova no app
instalado em cada plataforma antes de habilitar o recurso.
