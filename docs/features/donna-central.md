# A bot principal na barra lateral

## Berga pronta no Hub novo

Ao iniciar um Hub novo, o Berga já aparece como principal no Space pessoal,
com o avatar Donna e a autonomia **Pedir**. O motor inicial é Codex, com o modelo
padrão; nenhuma conta de IA é escolhida automaticamente. Conectores, logins,
skills e acesso a computadores começam desligados. A memória própria do motor
também fica desligada.

Depois de criado, o Berga é fixo ([contrato](donna-fixa.md)): ele não pode
ser apagado, renomeado nem trocado como principal, e as instruções base não mudam. Motor,
conta, ferramentas e as preferências do dono mudam só pelo app do dono; a aparência é livre.

A conversa traz um aviso do Hub com o caminho **Configurações do bot → Motor →
Gerenciar contas de IA** para conectar e escolher a conta. Sem uma conta pronta,
o Berga continua visível; sua criação não inicia a IA nem envia mensagem. Um
login antigo do Codex já configurado no Hub conserva o comportamento existente.

A inicialização é única e persistente: reiniciar não duplica o Berga, e renomear,
apagar ou desmarcar o principal não faz o padrão voltar. Hubs com bots, inclusive
excluídos ou ilegíveis, histórico ou configuração prévia de time são preservados.
Nome e aparência não concedem gestão: a principal é o ID salvo pelo Hub.
Esta criação não entrega a saudação automática nem altera conversas anteriores.

## Gestão após conteúdo externo

Só a principal configurada pelo dono pode criar ou alterar outros bots, em um
turno iniciado pelo dono na própria conversa. Rotina, webhook, canal ou pedido de
outro bot não autoriza gestão. A leitura da equipe continua disponível.

Se houver exposição externa conhecida, o Hub recusa a gestão mesmo em Auto e pede
ao dono que confira e aplique a mudança no editor de bots. Além da marca dos
conectores, contam anexos, colagem/citação, mensagens de outro bot e ferramentas
externas ou de origem desconhecida. As ferramentas internas `chat`, `memory`,
`manage` e `computers`, identificadas pelo adaptador e conferidas nas definições
que o Hub entregou à sessão, não criam exposição por sua própria chamada ou
resultado. Os avisos do Hub sobre ações do dono também não criam exposição.
Leituras externas por conectores ou computadores continuam marcadas. A marca
persiste entre turnos e reinícios e não é apagada por uma chamada interna. Sem procedência
claramente digitada, Auto exige o cartão síncrono existente.

O cartão com o antes e depois ainda será entregue numa etapa futura. Esta
barreira protege a ferramenta `manage`; o porteiro de permissões do motor e o
isolamento do Hub continuam trabalhos separados.

## Cartão da principal

A principal de cada Hub fica em um cartão fixo, centralizado acima das seções,
com avatar, nome, sinais de atenção e contador de mensagens não lidas. A identidade
vem da configuração do Hub, nunca do nome do bot. Clicar abre a conversa; clicar
de novo preserva a conversa aberta.

Ao abrir o app, a conversa da principal já vem aberta, sem passar pela tela de
boas-vindas; ela também volta quando a conversa aberta deixa de existir. Os
detalhes ficam em [Janela do Mac](janela-do-mac.md#ao-abrir).

O cartão compacto mostra **Ocupada**, **Precisa de você**, **Aguardando cota**,
**Não foi possível concluir** ou **Livre**, conforme o estado do atendimento.
Um pequeno ponto verde acompanha **Livre**; amarelo indica trabalho, espera por
cota ou pedido de atenção. Erros ficam em vermelho e falta de conexão em cinza.
Um pedido de aprovação tem prioridade e mantém o aviso de cota ou erro na segunda
linha, com o horário de retomada quando conhecido. Sem conexão, mostra **Sem conexão com o Hub** e rotula o último estado
conhecido. Não mostra a última mensagem nem inventa contagens de tarefas.
Se a versão for incompatível, mostra **Atualize o app para ver a principal**.
Quando o Hub não comprova a audiência do runtime neste Space (`status: "unavailable"`),
o cartão mostra **Estado indisponível** com um anel vazado no lugar do ponto, sem
horário, motivo ou lugar; não é Livre nem Ocupada ([Spaces](../reference/spaces.md#audiência-do-runtime-no-botjson)).

No topo da conversa, a principal tem um avatar maior, dentro do espaço já
reservado ao cabeçalho. Durante a resposta, o símbolo oficial da Mexerica pisca
ao lado de **Escrevendo…** e do tempo decorrido. A animação respeita a preferência
de reduzir movimento e fica parada quando o bot precisa de uma resposta sua.

Busca, seções recolhidas e filtros de seção não escondem o cartão. O filtro de
computador controla quais Hubs aparecem; cada Hub visível tem seu próprio cartão.
A busca geral também encontra a principal, inclusive em snapshots antigos que
ainda a marquem como oculta. No trilho compacto, o avatar permanece no topo, com
status, contador e resumo acessível.

O menu **…** oferece **Marcar como lida**, **Editar perfil** e **Recomeçar a principal…** (recomeço legado da
conversa principal, sem resumo). Com um Hub que anuncia sessões, também **Nova sessão**, que cria uma
sessão extra e a abre.
**Abrir tarefas** aparece desabilitado, com **Tarefas ainda indisponíveis neste Hub**,
até a integração do painel. Mover, fixar, ocultar e apagar não fazem parte do menu.
As seções e posições salvas dos outros bots não são alteradas. O Berga é sempre
o principal e fica no topo da equipe.
O app guarda localmente a última identidade confirmada por computador e Hub.
Antes de confirmar novamente, esse valor só mantém o principal fora da lista e
da navegação e protege suas ações. Ele é descartado se a identidade do Hub mudar
e nunca habilita o seletor.

Quando o Hub confirma que não há principal, o cartão só informa o estado. O app não
escolhe nem troca o principal: o seletor "Escolher a bot principal" saiu, e o
Hub recusa pela API qualquer principal que não seja o Berga que ele mesmo criou. A correção persistida de snapshots legados com `hidden`
e a recusa de ocultação por outras APIs pertencem à próxima etapa do Hub.
