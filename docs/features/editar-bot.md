# Editar um bot

Nome e instruções do próprio bot são escolhas do dono em Configurações. Pedir no
chat não dá ao bot permissão para reescrever seu perfil. Essa regra inclui Berga
e qualquer futuro bot criador. A gestão autorizada de outros bots continua com
as aprovações, os avisos e o Desfazer descritos em [colaboração](bot-collaboration.md).
Modelo, autonomia, Berga e exclusão continuam sendo escolhas exclusivas do dono.

## Configurações em abas

**Configurações do bot** no menu do bot ou no painel da conversa abre a mesma
página na área de conteúdo. **Abrir conversa** retorna ao bot que está sendo
editado. Canais continuam com suas próprias configurações.

A lista lateral troca o conteúdo: só a aba escolhida fica visível, marcada, e começa
no topo. Use ↑/↓ para trocar de aba, Home/End para a primeira/última e Tab para
entrar no painel; o foco fica visível. O estado **Salvo** pertence à aba aberta,
assim como **Salvando** e alterações pendentes. Erro, conflito ou recusa marca a
aba de origem; enquanto houver erro em outra aba, o status geral avisa e nunca
anuncia **Salvo**. Identidade e Motor também mostram a aplicação pendente
informada pelo Hub. Uma operação de outra aba
continua em andamento; se ela falhar, a marca e o status geral orientam a
voltar à aba de origem. Áreas sem gravação nesta
página não anunciam **Salvo**. Os rascunhos permanecem ao trocar de aba.

A navegação tem Identidade, Motor, Comportamento, Ferramentas, WhatsApp (quando o
plugin está ligado), Memória, Voz,
Notificações, Rotinas, Exportar e Zona de perigo. **Identidade** edita o perfil;
**Motor** mantém o campo de motor (na OpenCode, com o aviso de segurança, sem bloqueio) e
escolhe o modelo, o esforço e a velocidade pelo **seletor único da Conversa**, o
mesmo do compositor e do Novo bot, no modo "padrão do bot": favoritos e atalhos ⌘1 a ⌘3
deste aparelho, a linha **Padrão do motor** e nenhum "Default (recommended)". O seletor só
lista o motor salvo; trocar de motor continua pelo campo Motor, com a confirmação dele. Sem
conta, a escolha grava pelas mesmas intenções de antes (`model`, `modelOptions`, `fastMode`);
com conta, só os modelos que a conta comprovou, gravados pelo editor versionado, como antes.
Conta sem lista comprovada mostra a nota da conta, sem seletor; sem autorização vigente para a
conta, o seletor fica desativado. Motor que pede entrada mostra uma nota no lugar do seletor.
O interruptor **Modo rápido** só aparece quando o seletor não oferece a velocidade (por
exemplo, para desligar ou ver a recusa do motor). Esse é o **padrão do bot**, e o aviso
**Vale para conversas novas** fica sempre embaixo: mudar o padrão não troca o modelo de
nenhuma conversa aberta, nem da principal. Ao lado, **Usar também na conversa
principal** leva o padrão salvo à principal pelo mesmo pedido da troca no compositor
(`sessionEngineSet` com `sessionId: null`, revisão lida da sessão, `fastMode` booleano). O
botão só aparece quando o Hub anuncia a troca por conversa (ação `engine` em
`sessionActions`) e manda a projeção `engine` da principal; sem isso, só o aviso, e o app
nunca grava a principal por outro caminho. Se a principal já usa o padrão, o botão fica
desativado. Numa conversa grande (50 mil tokens ou mais a reler, no mesmo provedor), pede a
mesma confirmação de troca antes de mudar. Recusas do Hub (Berga fora do app do dono,
revisão mudou, modelo não liberado) aparecem em texto leigo; quem decide é o Hub;
**Comportamento** mantém Pedir/Liberar, o seletor de pasta do Hub (Workspace),
**Usar o computador** (veja abaixo) e o grupo **Navegador**, que escolhe a
preferência do dono entre **Meu computador** (o navegador que já está no
computador autorizado, pelas ferramentas de computador existentes), **Integrado
ao app** (indisponível hoje, com o motivo) e **Desligado** (sem navegador: o bot fica sem
navegador e as outras ferramentas não mudam — "Usar o computador" não é afetado
e a opção aponta para o controle, logo acima, para quem quiser tirar também
esse uso). Sem
liberação de "Usar o computador" ou sem tela admitida, "Meu computador" explica o
que falta e aponta o caminho, com o nome real da máquina; não há troca silenciosa
de destino em nenhuma direção;
**Ferramentas** permite ligar e desligar conectores, logins e skills.
Cada skill é uma escolha própria: o app envia só aquela skill, presa à
seleção e à instalação vistas no clique. Se outra janela, aparelho ou remoção
mudou a lista, o Hub recusa e a tela pede para conferir e escolher de novo; se a
resposta não chegou, a tela diz que não confirmou, sem reenviar. Salvar o resto
do perfil nunca leva a lista de skills. A mesma escolha, por bot e por skill, também
fica em Extensões → Skills → **Base central**;
**Notificações** mantém o interruptor de avisos. As demais entradas indicam uma
próxima etapa, sem gravar opções.
Em uma janela estreita, use o menu **Abas das configurações**. Os chips do cabeçalho levam à seção
correspondente. Pedir/Liberar descrevem a autonomia atual; os três níveis com
regras da etapa seguinte ainda não estão disponíveis.

Em Identidade, edite **Nome**, **Função** (a etiqueta da barra lateral, até 40 caracteres;
veja [Novo bot](criar-bot.md)), **Instruções permanentes** e **Aparência**. O editor
de avatar oferece Função, Corpo, Olhos, Cor, Acessórios e Selo. A função do avatar
é aparência; não concede gestão nem muda o papel no time. Papel descritivo e
senioridade dependem de suas próprias capacidades e não têm campos nesta etapa.
O app não escolhe nem troca o bot principal: o cartão "Bot principal (Donna)" saiu das
Configurações de todos os bots, e o Hub recusa trocar o principal pela API (`donna_fixed`).
Com o Berga definido, ele é fixo ([contrato](donna-fixa.md)): a página
avisa que nome, instruções base e papel de líder não mudam, mostra Nome, Função e Instruções base
como texto de leitura, sem caixa de edição, com uma explicação curta; oferece o campo "Suas preferências para o Berga" e mantém Aparência, Motor,
Comportamento, Ferramentas e Notificações para o dono. Não oferece apagar. O Hub recusa o resto com `donna_fixed`, mesmo que outro cliente tente.

Texto é salvo após uma pausa de 600 ms ou ao sair do campo. Aparência é enviada
ao escolher. Só os campos alterados são enviados, com uma requisição por vez;
uma resposta atrasada preserva o que você digitou depois. **Salvo** aparece após
a confirmação do Hub. Instruções novas valem no próximo turno.
Espaços e quebras de linha finais permanecem no campo durante a digitação;
compará-los após aparar evita enviar alterações só de espaço. Corrigir um campo
inválido reativa o salvamento automático.

### Usar o computador

O bloco **Computador** de Comportamento segue a tela que o Hub informa (`screen.computerUse`: no Windows
sempre; no macOS e no Linux, com a tela ligada nesse computador). Não dependem
mais de um host neste Mac:

- **Com tela:** o interruptor aparece, com "Ver a tela e usar mouse, teclado e
  aplicativos (como o navegador) do computador onde seus bots trabalham.". Se o
  ajudante de tela ainda não tem permissão de captura e controle, a dica sugere
  liberá-la nesse computador; a dica nunca esconde o controle.
- **Bot ligado sem tela:** o interruptor continua, ligado, com "Ligado, mas o
  computador onde seus bots trabalham não tem tela agora. Você pode desligar.".
  Um bot ligado nunca perde o interruptor, porque o Hub continua entregando a
  ferramenta a ele.
- **Sem tela e desligado:** sem interruptor; uma linha diz que a tela se liga no
  próprio computador e **Ver seus computadores** abre Ajustes → Computadores.
  Essa página mostra os computadores; não liga a tela (`setScreenEnabled` é só
  do próprio computador).

No editor e no painel, o controle envia só `{computer}` em `updateBot` e espera
a resposta: o valor mostrado é sempre o confirmado pelo Hub, que chega pelo evento
do bot. Enquanto o pedido está em curso, o interruptor fica em leitura. Uma recusa
mantém o valor confirmado (o interruptor não some) e aparece junto ao controle.
Cliente local e dispositivo com controle podem mudá-lo; executor é recusado. Fora
de **online** (conectando, sem par, desconectado) ou com versão divergente, o
controle fica em leitura. Ao desligar sem tela, depois da confirmação, o foco vai
para **Ver seus computadores**.

Os demais controles salvam somente sua intenção na hora, após confirmar o texto
pendente. Autonomia usa a escolha explícita do dono por bot. Nenhum desses
controles envia o rascunho integral ou altera campos de identidade junto.
Escolher **Comando próprio** abre o campo localmente. O motor só é gravado ao
sair do campo com um comando preenchido. Voltar ao motor salvo ou escolhê-lo de
novo preserva o modelo, as opções e o comando, sem enviar alteração ao Hub.
Uma recusa do Hub aparece junto ao controle, com o texto recebido, e é anunciada
ao leitor de tela; agir novamente nele ou salvar a identidade limpa o erro.
Enquanto houver uma recusa, o cabeçalho avisa **Uma alteração foi recusada; veja
a seção**, em vez de **Salvo**.

Com o Hub desconectado, os campos ficam em leitura e o rascunho permanece na
página. Ao reconectar, o editor relê a configuração e pede **Tentar novamente**
para enviar o rascunho pendente. Erros mantêm o texto e oferecem nova tentativa.
Se aparecer **Este bot mudou em outra janela**, copie o texto que deseja guardar
ou continue editando; **Recarregar configurações** pede confirmação antes de
descartar o rascunho e trazer o estado do Hub. O editor nunca sobrescreve o
outro cliente automaticamente.

Fechar as configurações, abrir a conversa ou trocar de bot conclui o save. Se isso não for possível,
**Alterações não salvas** permite continuar editando ou descartar. Fechar a janela
do app só a esconde e mantém a página. Sair do app ou instalar uma atualização
não é bloqueado por um aviso de navegador; texto ainda não confirmado pode se
perder ao encerrar o app.

## API parcial do editor

Os métodos são intenções de cliente autenticado, sem registro como ferramentas MCP:

- `donnaBotEditor {botId}` lê o perfil canônico e uma `settingsVersion` opaca.
- `donnaBotEditorSave {botId, expectedVersion, patch}` salva apenas `name`,
  `description` e `avatarConfig`. As capabilities informam essa lista e as áreas
  ainda indisponíveis. O contrato do desktop é `shared/fork-bot-editor.ts`.

Cliente local e dispositivo com controle autorizado podem usar os métodos.
Executor e canal de pareamento são recusados. Não existe `actor` no corpo do
pedido; nenhum campo enviado atribui autoria humana. A autenticação local mantém
o limite conhecido do usuário do sistema compartilhado: não prova a presença
de uma pessoa contra acesso indevido ao banco/token pelo mesmo usuário Linux.

O perfil retornado contém somente nome, instruções e avatar. Não inclui comandos,
credenciais, contas, caminhos ou transcript. Custo, velocidade e motor efetivo
retornam `null`; contas retornam uma lista vazia até a entrega dessas capacidades.

O Hub valida todo o patch antes de escrever. Nome é aparado, tem de 1 a 40
caracteres e recusa nomes reservados. A mesma validação de nome vale na criação e
atualização pela gestão e na nomeação automática. Instruções alteradas têm até
4.000 caracteres; abrir ou renomear não trunca instruções antigas maiores. Reenviar
nome ou instruções iguais após aparar espaços não regrava o campo, mesmo que o
valor legado exceda esses limites. Tipos e `null` continuam sendo validados.
Avatar reutiliza o schema
da Mexerica. Campo desconhecido, tipo inválido ou `null` indevido resulta em
`invalid_field`, sem save parcial. Modelo, autonomia, gestão, pasta, conectores e
exclusão não são aceitos nesse patch. Os controles existentes usam intenções
parciais separadas de `updateBot`; autonomia inclui `permissionChosenByUser`.
Uma pasta
legada, recusada ou que sumiu aparece como aviso em `workspaceIssue` na leitura
e no retorno do save, sem travar mudanças de nome, instruções ou avatar. O caminho
salvo é preservado até a escolha de outra pasta nas configurações.

Falhas ao salvar usam códigos estáveis com mensagem em português, sem caminhos ou
texto interno: `folder_unavailable` para pasta indisponível/recusada,
`model_unavailable` para motor desconhecido, `command_not_allowed` para comando
recusado e `save_failed` para outra falha. Uma recusa anterior à persistência
conserva o perfil salvo.

A versão é um hash da configuração, incluindo avatar e a identificação do Berga,
independente da revisão de progresso. O Hub compara a versão sob o mesmo lock
usado na gestão. O app relê ao mudar a configuração ou a principal, inclusive ao
fixar, reordenar ou ocultar o bot; mudanças só de status ou atividade não relêem o
editor. Edição por outro cliente ou caminho legado invalida a versão;
`settings_conflict` exige reler e resolver o conflito. Bot apagado não é recriado.

Um save confirmado retorna `application: "next_turn"` e **Aplica no próximo turno**.
Se o perfil foi persistido e o ator está indisponível, retorna o perfil salvo com
`application: "apply_pending"` e **Salvo, aplicação pendente**. Ler novamente ou
repetir o patch com a versão retornada reenvia a configuração canônica ao ator,
sem regravar um perfil idêntico. O Hub restaura os atores do banco ao reiniciar.
Receber a reconfiguração na fila confirma aplicação no próximo turno, não uma
mudança em um turno já em execução.

### Motor: escolhas disponíveis

A aba mostra a conta, o modelo e seus ajustes. Uma conta de API paga sem
conexão aparece como **Por chave**; a cobrança conectada continua sendo
**API paga**. **Configurar por comando** conserva
o comando próprio já existente. Sem escolher uma conta, o bot usa o acesso já
configurado no Hub. Sem lista de modelos da conta, usa o modelo escolhido pelo
serviço. Abaixo, **Escolha de conta e modelo** lista **Escolha automática**,
**Contas alternativas (reservas)** e **Guias de escolha** apagadas, com o selo
**ainda indisponível** e uma frase do que farão. Não são botões nem campos: não
recebem foco, não gravam e não chamam o Hub. Por enquanto, as conversas novas usam
a conta e o modelo escolhidos acima; cada conversa aberta mantém o modelo dela. A conferência de roteamento continua fora da tela. As
preferências antigas continuam guardadas, não aparecem como se valessem e não
alteram a execução nem a autoridade do Hub. Veja [Roteamento de modelos](roteamento-de-modelos.md).
