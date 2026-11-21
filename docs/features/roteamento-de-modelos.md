# Roteamento de modelos

## Tela atual

**Configurações do bot → Motor** mostra a conta, o modelo e os ajustes já
funcionais. Abaixo, a escolha automática, as contas alternativas (reservas) e os
guias de escolha aparecem apagados, com o selo “ainda indisponível” e uma frase do
que farão; não têm controle, não gravam e não chamam o Hub. O rodapé diz que, por
enquanto, as conversas novas usam a conta e o modelo escolhidos; cada conversa aberta
mantém o modelo dela. A conferência de
roteamento continua fora da tela. O desktop não lê nem grava essas preferências ao
abrir as Configurações. Preferências guardadas anteriormente permanecem no Hub,
não são exibidas como se valessem e não ativam seleção automática, rodízio ou
alternativas.

## Contrato preservado no Hub

A escolha fixa continua sendo a conta, o motor, o modelo e as opções escolhidos
para o bot. Bot novo começa com essa escolha e sem reservas. A conferência
estática no cadastro não chama o modelo nem comprova cota ou resposta do serviço.
O cadastro comprova contas do dono, mas não sua filiação a outro Space ou pagador.
As fronteiras de autorização, a prioridade do guia do bot dentro do Space e a
ordem das reservas guardadas permanecem no Hub, sem uso automático. Guias não
concedem ferramentas, acessos ou permissões. Uma alternativa futura só poderá
ser considerada antes do primeiro envio de uma sessão nova, nunca no meio dela.
Gestão das contas exige autorização vigente; Control ou gestão de aparelhos,
sozinhos, não bastam. Não existe ferramenta de roteamento para bots.

## Entregas posteriores

O recibo/checkpoint do Roteamento está ligado ao despacho de turnos humanos pelo
Sessões, na mesma transação da passagem `queued` → `sent`.
Ele guarda a combinação que de fato executa: a seleção EFETIVA da conversa do Pessoal
(principal ou extra), a escolha da conversa na principal de Space e o carimbo da
própria sessão. Cada conversa do Pessoal nasce com o retrato do padrão do bot. A troca do
dono (`sessionEngineSet`) grava a PRÓXIMA seleção; ela só vira efetiva no início de um turno
humano novo, nunca durante um turno, uma espera de limite, um retry de preparo ou uma
retomada. Rotina e canal seguem o padrão do bot, com slot e vínculo próprios. A retomada só lê o registro original, sem nova
decisão; erro de leitura falha fechado. Bot fora do Pessoal, sem conta gerenciada ou
na principal com conta gerenciada sem modelo explícito despacha sem roteamento, como antes; a extra com conta
válida aceita esse padrão sem opções e continua roteada, com `model: null` nas seleções pedida e admitida do
recibo. Esse valor registra a escolha implícita, sem identificar ou inventar o modelo usado pelo motor. Conta
gerenciada quebrada falha fechado. Cada turno
fica classificado como roteado ou não roteado no próprio registro de admissão;
turnos e esperas anteriores ao gravador ligado são não roteados por construção e
retomam como antes. Auto, reservas, Decision e
rodízio continuam desligados; ainda não há controle novo no app.

O app já pode ler o recibo de um turno (só o dono, com a mesma autorização
de contas de IA das preferências): qual conta, motor, modelo e opções foram pedidos e
admitidos, quem decidiu e por quê. O recibo não mostra token, vínculo, carimbo nem
dados privados da origem. Uma mensagem ainda na fila aparece como "pendente"; uma
mensagem já enviada sem recibo (não roteada ou anterior ao Roteamento), como "não
roteada"; recibo ilegível ou mensagem de outro bot/sessão é erro, nunca "sem recibo".

**No app:** a resposta que abre cada turno do bot mostra um pequeno
ícone de recibo depois da hora. Clique nele (ou Tab até ele e Enter/Espaço) para ver
o modelo, a conta (pelo nome da lista de contas de IA; conta que saiu da lista aparece
como "Conta removida", sem o id), o motor, o esforço e o motivo: "Fixo do bot", "Escolha
desta conversa" ou "Fixada nesta sessão". No Fixo há o atalho "Abrir Motor de <bot>".
Esc fecha e devolve o foco ao ícone. O nome do modelo aparece ao lado do ícone só quando
muda em relação ao turno anterior da conversa. Resposta sem recibo não ganha ícone; o
mesmo quadro abre por Mais ações (⋯) → "Como foi respondida", que explica que o bot usou
a configuração do próprio motor e que isso não é erro. Na fila: "Ainda não enviada".
Falha de leitura: mensagem curta e "Tentar de novo". Sem a autorização de contas de IA,
ou com um Hub que ainda não tem o recibo, os ícones não aparecem. Auto, Reserva e Dono
não aparecem como disponíveis.

O recibo **mostra a combinação escolhida no envio; não confirma o que o motor usou por
dentro**. O app lê o recibo da mensagem do usuário do mesmo turno, uma vez por mensagem,
e guarda em memória (fora do espelho). O topo da conversa mostra só a seleção configurada
para os próximos turnos (`session.engine`), com "Padrão inicial do bot" ou "Escolhido nesta
conversa"; sem projeção legível, "Modelo indisponível", sem copiar o padrão do bot.

O carimbo do bot conserva somente conta, revisão e motor; ele não comprova
modelo/opções executados. A captura de Space tem sua fonte própria. Fora de Space, a
sessão com seleção efetiva própria (extra ou a principal do
Pessoal) usa a sua, `account.stamp.<bot>/session:<id>`, com o mesmo formato do carimbo do bot
e sem cair para ele; a principal sem seleção (fora do escopo novo ou anterior à migração)
continua no carimbo do bot. Ao gravar, o Hub confere que a seleção efetiva é igual à
admitida, com conta (roteado nunca aceita conta nula). O carimbo de um domínio não
invalida a sessão de outro: trocar a principal não derruba rotina/canal, e o padrão do bot
não derruba a principal com seleção própria. Ler um registro que não
existe devolve "ausente"; registro corrompido ou divergente é erro. Ausência não
autoriza executar, repetir turno nem decidir de novo. Na retomada, cada turno traz a
marca "roteado" ou "não roteado" decidida na admissão; marca ausente ou que contradiz
o registro pausa o turno de forma visível, sem cair no caminho antigo. Cobertura
antes/depois e identidade/geração/catálogo amplos sem produtor permanecem como
**fato requerido não atestado**. Guardar esses dados não autoriza executar um
turno que dependa dessas provas, nem comprova o modelo observado na execução.
O despacho já grava o recibo; Auto e o restante dependem das próximas etapas.

Sessões, escolha efetiva por turno, seletor Decision, Auto, reserva executável,
rodízio, troca manual e limites técnicos ainda não estão disponíveis.
As proteções continuam sendo as
que cada motor já oferece; esta configuração não amplia sua cobertura.

O futuro **Testar agora** será
manual do dono, com aviso de cota e resultado privado datado; não dará concessão
nem garantia permanente e não usará conteúdo do dono no prompt. Nenhum
botão, chamada ou resultado de teste real integra esta etapa.

Se o cadastro de contas não puder ser lido, a tela mostra o erro; isso não é
tratado como “nenhuma conta”. Guias, reservas e rascunho continuam preservados.
Uma tentativa de salvar nessa condição não apaga referências nem grava só uma
parte da configuração. Outros Spaces continuam sem consultar o cadastro global.

A indicação de conferência acompanha a combinação salva de motor, conta, modelo
e opções. Ao mudar essa combinação, confira novamente; enquanto houver mudança
pendente, a tela aguarda salvar ou resolver as configurações. Salvar guias e
reservas continua sendo uma confirmação separada, com o rascunho preservado.
