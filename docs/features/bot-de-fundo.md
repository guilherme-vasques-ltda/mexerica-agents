# Bot de fundo — painel de configurações

A página começa dizendo o que é o Bot de fundo e agrupa as funções
pelo campo `consumer` que o Hub já manda: **Em uso** (`background_worker`: Memória e
Títulos), **Em preparo** (`awaiting_implementation`: a escolha fica guardada; recolhido atrás de "Ver N em preparo") e **Voz**
(gerenciadas na página Voz). Hub sem `consumer`, ou com valor desconhecido, mostra as funções
num grupo neutro **Funções**, sem afirmar que rodam.
Cada função abre a escolha na própria linha; o estado aparece numa frase ("Pronto: a
conta e o modelo estão disponíveis" ou o motivo). O botão Conferir saiu: cada leitura
já traz a conferência. O computador só aparece quando há mais de um.

Escolha o computador e a função. Personalizado conserva conta, modelo e opções
como um conjunto; o esforço usa o valor anunciado, inclusive max. O painel abre
um rascunho com a revisão lida. Salvar exige essa revisão; conflito ou salvamento
sem resposta confirmada pede uma nova leitura antes de outra alteração, sem
reenviar automaticamente. Troca de computador, posse ou conexão invalida o estado
anterior. Os eventos apenas pedem nova leitura; não há um segundo catálogo no app.

O cartão "Neste Space" mostra só a seleção do Space ativo (cada Space tem as suas configurações) e vincula a seleção Memória existente por referência e revisão; as dos outros Spaces seguem salvas no Hub.
Autorização cadastrada, autorização desatualizada e execução indisponível são
estados diferentes. Ausência de API não apresenta vínculos ou controles ativos
inventados. Essa API de vínculo usa Local ou a capacidade original engine.accounts
com a guarda SAAS-04, enquanto functionModels/save/check continuam Local. Bots
não recebem essas operações. A cerca do executor permanece no Pessoal sem contexto
até haver prova de conta no próprio emissor; conferir cadastro não fornece essa prova.

**Ajustes → Atividade** (página própria; antes ficava no fim desta) consulta `backgroundHistory {}` do computador conectado, somente
no acesso Local, com até 50 registros na ordem fornecida. O status é obrigatório;
“Resposta recebida” não confirma aplicação. Modelos efetivos podem não ter sido
informados; etapa pode ser nula e uso é opcional, restrito aos quatro contadores
de tokens. Ausência não vira zero. Não há custo, conta ou causa detalhada inferidos.
Na primeira carga, sem dados, aparece "Lendo a atividade…". Numa recarga, as linhas da
última leitura continuam na tela e o ↻ vira um indicador de carregando até a resposta; se a
recarga falhar (erro ou recusa Local), as linhas ficam e um aviso em cima diz o erro
("Mostrando a última leitura."). Troca de computador ou de conexão começa do zero, sem as
linhas da conexão anterior. O painel não usa dados de outra API para completar o histórico.
Cada registro é uma linha compacta (hora, função, bot pelo `botId` do registro, status,
tokens de entrada + saída), agrupada por dia, com filtro Todas | Com falha; clicar abre
modelos, os quatro contadores e a etapa da falha. O Hub guarda no máximo 50 e acrescenta no fim; a
Atividade mostra o mais novo primeiro, em páginas numeradas de 10 ("X–Y de N"), e o filtro volta
à página 1. No paginador, ← → mudam de página (espelhadas no RTL), Home e End vão às pontas, e o
foco acompanha o número da página.
Falha de seleção repetida em **Organizar memória** (conta desconectada, sem modelo
— inclusive escolha personalizada cujo modelo ou opção deixou de ser anunciado —,
sem conta Claude, Codex ou API) ocupa uma linha só: enquanto o motivo e a
configuração são os mesmos, o Hub só atualiza o horário da linha de etapa `selection`.
As novas tentativas esperam 5 s, 10 s, 20 s, 40 s e param após 5 falhas iguais
seguidas; falhas simultâneas de bots diferentes na mesma espera contam uma vez e usam
a mesma linha. A saída da parada é por evento: toda gravação do cadastro de contas
(conectar, reconectar, catálogo novo, inclusive contas gerenciadas) e todo salvar da
função zeram a espera na hora quando a seleção mudou, e o sonhador volta a tentar em
até 60 s. Como reserva para o que muda fora do Hub, parado, ele sonda no máximo a cada
10 min só a conta escolhida para a função (gerenciada pelo catálogo das contas
gerenciadas; login local pelo catálogo de fundo, mesmo com Haiku já anunciado). O motivo chega ao dono como aviso na
conversa do bot, com o rótulo da conta (ex.: "Organizar memória: a conta {rótulo} está
desconectada; reconecte em Contas de IA ou escolha outra em Bot de fundo > Modelos por
função"), sem repetir enquanto o texto for o mesmo, e também em `functionModels`
(`resolution.unavailable.reason` e `.message`). Um campo próprio de motivo na linha da
Atividade fica para uma atualização futura, junto com a subida do `MIRROR_SCHEMA`.
Quando a assinatura atinge o limite durante a execução de **Organizar memória**,
a tentativa seguinte espera 15 minutos; novas recusas ampliam a espera até uma hora.
Um novo turno não encurta essa espera. O executor ainda não preserva o horário de
liberação, então a retomada é uma nova tentativa, não uma garantia de limite renovado.

A migração da função Decision e seu serviço ficam para uma etapa futura: não
há roteamento ativo por esse painel.

## Conector e conta indisponíveis

A descoberta do catálogo do trabalhador e as tarefas de Memória e Títulos usam
exclusivamente `@agentclientprotocol/claude-agent-acp@0.94.0`, versão auditada
por leitura de código. O nome e a versão exatos são conferidos antes de abrir
sessão ou enviar conteúdo. Outra versão, inclusive 0.100.0, permanece recusada
até auditoria própria; atualizar para uma versão mais recente não basta. Não há
alternativa por executável ACP do PATH, consulta à versão mais recente do registry
ou edição de seu cache. O operador precisa provisionar o pacote fixado antes do
uso; download durante o início pode consumir o prazo da conexão. O pin é restrito
a esse fluxo e não muda o conector das conversas normais.

O pin corrige a escolha do conector, mas não comprova conta conectada, identidade,
assinatura, cota ou catálogo disponível. Uma assinatura com prova do plano, mas
sem identidade de cota, data de verificação e modelos, continua indisponível.
As guardas de política local/MDM, espaço, conta, identidade, cota, origem,
ferramentas e hooks permanecem. Também permanece o limite aceito para políticas
remotas do Claude, descrito no [manual do harness](manual-do-harness.md).
Fixtures comprovam o contrato enviado pelo Hub; não comprovam o motor real,
políticas empresariais nem funcionamento de uma instalação.

No painel, Automático sem conta e sem vínculo informa: “Nenhuma conta está
disponível para esta função.” Para Memória e Títulos, o próximo passo é conectar uma conta
Claude por assinatura no Pessoal, nas Configurações do sistema. Uma conta
explicitamente escolhida que deixou de existir continua com o motivo próprio
de conta não cadastrada. Essa distinção usa a seleção recebida, sem reset,
troca de destino, migração do banco ou novo cache. Salvar ou conferir não
comprova execução. O custo continua informativo; o teto de gasto do Hub, desligado por padrão, pode travar o trabalho autônomo — ver [Teto de gasto](teto-de-gasto.md).

## Escolha de conta e modelo por função

Em Ajustes → Bot de fundo, cada função escolhe conta e modelo no mesmo seletor do
compositor do chat (`SeletorModelo`, modo `funcaoDeFundo`; sem segundo componente).
A conta faz parte da escolha: o campo mostra “Motor · Conta: …”, com nomes únicos.
Conta desconectada, com login por confirmar, indisponível, sem modelo informado ou
(Memória e Títulos) sem assinatura confirmada fica apagada na linha Conta, com o estado
em palavras e a ação que já existe (Conectar, Confirmar login), e nunca vem escolhida.
O motor que a função não aceita fica no trilho, apagado, com o motivo: Memória e Títulos
só rodam no Claude (`background_selection::resolve` recusa Codex e chave de API).
“Automático” é o modelo vazio (receita do catálogo do Hub). A frase de resumo usa valores
do código (uma rodada por bot por dia, bot parado há 30 min — `memory_auto/sleep.rs`) e
o rodapé diz “Vale a partir da próxima rodada. Nada muda nas conversas.” Só o Space ativo
aparece. Funções por Space seguem aguardando `spaceBackgroundContract` do Hub.

## Medição por trabalho

Contrato futuro para Memória, Títulos e Decision: recibo de uso ligado ao pedido,
trabalho/tentativa, modelo efetivo e conta opaca autorizada. Cada valor distingue
informado de estimado; ausência não vira zero nem preço inventado. O vínculo
não permite ler o pedido ou os dados privados da conta em outra audiência.

Um status response_received/applied ou um retry de leitura não cria outra cobrança
no resumo. Uma nova tentativa real tem identidade própria; aplicação/reconciliação
atualiza o mesmo recibo por chave estável, sem duplicar. Conta/modelo/uso só são
atribuídos quando comprovados; estimativa nunca é promovida a informação do
provedor. A definição exata de métricas, unidades, cache e reconciliação será
fechada antes de implementar, sem somar contagens sobrepostas.

É informativo; fora o teto de gasto do Hub, desligado por padrão (ver [Teto de gasto](teto-de-gasto.md)), não há orçamento imposto pelo Berga. O histórico atual continua com seu
histórico curto Local e uso opcional em tokens; não entrega esse recibo durável
por pedido/conta nem estimativa de custo. A integração futura preservará autoridade,
audiência e migração compatível, sem outro catálogo de contas ou modelos.
