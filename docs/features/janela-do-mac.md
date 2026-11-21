# Janela do Mac

Como a janela de conversa do app do Mac se organiza.

## Ao abrir

- O app abre direto na conversa da **Berga**, a principal do Hub que a faixa mostra (o do bot selecionado ou, se só um Hub tem spaces, ele; senão o primeiro computador online da barra). Enquanto a identidade da principal é consultada, a última principal confirmada neste Hub já abre; se a consulta trouxer outra e você ainda não escolheu nada, a conversa passa para ela.
- Sem principal (ou com a consulta falhando e nenhuma principal guardada), abre a primeira conversa direta com bot da barra; um canal só quando não há nenhum bot.
- Enquanto a consulta da principal ainda corre, o lado direito fica neutro, sem a tela de boas-vindas. "Conectando aos seus computadores…" aparece enquanto conecta, sem botão. Com a lista vazia, o lado direito também fica neutro: os personagens com **Novo bot** só aparecem depois de 5 segundos com todos os computadores mostrados online sem nenhum bot visível chegar, ou antes, se o Hub disser na conexão que não há bot nenhum. Os 5 segundos recomeçam a cada reconexão, inclusive depois de uma queda curta. Um bot oculto guardado no app não conta como visível até o Hub mandá-lo de novo visível; uma principal que ficou oculta numa versão antiga não abre sozinha, mas continua em Buscar. Vale igual para Hub antigo, sem spaces. Sem nenhum computador da barra online nem conectando, o lado direito mostra **Sem conexão com o Hub**, o computador onde os bots continuam, a mensagem real da conexão, **Tentar de novo** (a mesma reconexão do ⌘R) e **Ver computadores**; a lista não diz "Seus outros bots aparecem aqui" enquanto isso.
- Queda curta: se o túnel SSH ou o canal da nuvem cai, a conversa aberta, a lista, o rascunho e a rolagem ficam; a faixa **Sem conexão — reconectando…** com **Tentar agora** substitui o selo de conexão, enviar e aprovar ficam desligados com o motivo, e a reconexão é automática. A tela de desconectado só aparece depois de 5 minutos sem link ou numa falha definitiva.
- Se a conversa aberta some (bot apagado ou ocultado, computador escondido no filtro, Hub removido ou trocado), o Berga abre no lugar. Uma queda passageira da conexão mantém a conversa aberta, com o último estado, mesmo que outro Hub online passe a ser o da faixa: a conversa não troca de Hub. Um bot de um computador escondido no filtro, aberto de propósito pela bandeja ou pela ilha, continua aberto enquanto existir e não for ocultado.
- O que você abre por conta própria (clique na barra, notificação, Buscar, editor, Rotinas, Ajustes) nunca é trocado, inclusive quando você reabre (ou clica no Berga) a mesma conversa que o app tinha aberto sozinho ou quando uma notificação chega antes de a barra aparecer (por exemplo, com o acesso ao computador na tela), mesmo de um computador escondido no filtro: a escolha automática só age quando não há conversa válida aberta, e espera enquanto um space, o editor, a criação, Rotinas ou a paleta estão abertos. O app não lembra o último bot aberto.

## Faixa (60 px, à esquerda)

- **Pessoal**: sempre o primeiro; o clique leva (ou volta) ao Pessoal, de um space ou de Rotinas, com o rascunho e a conversa como estavam. A barrinha na borda marca o lugar atual.
- No pé: **Rotinas** (calendário de rotinas), **Conectores** (ícone de grade; a tela de Extensões na página Conectores) e a **engrenagem**: Pendências (só quando há aparelho esperando aprovação, com ponto âmbar na engrenagem), Uso, Skills, Conectores, **Conta e suporte** (o menu da conta: Uso, Suporte, Ajustes e, com a nuvem ligada, entrar/sair) e Ajustes.
- **Spaces**: depois do Pessoal vêm os spaces do Hub (o do computador selecionado ou, se for o único com spaces, ele), cada um com o seu tile.
  - Clicar num tile abre o space: à esquerda, as conversas dele (a fixa com cada bot primeiro, as fechadas recolhidas); à direita, o histórico da escolhida (ao abrir, a conversa fixa com o Berga; sem ela, a primeira aberta), só para ler, e **Como está rodando** (como a conversa começou e se há conta ligada). `⌘1` volta ao Pessoal e `⌘2`…`⌘9` abrem os spaces na ordem.
  - O Pessoal na faixa, `⌘1` ou escolher um bot (Buscar, notificação) voltam à barra lateral e à conversa do Pessoal como estavam.
  - **Editar space…**, **Mover para cima** e **Mover para baixo** ficam no clique secundário do tile e no "…" do space aberto; o **+** abaixo cria um space, com nome, cor e ícone, letras ou logo.
  - Com muitos spaces, a lista rola dentro da faixa (roda, trackpad ou Tab), com as bordas esmaecidas. Pessoal, **+**, Rotinas, Aplicativos e a engrenagem continuam sempre à vista.
  - O Hub guarda e confere tudo. O formulário fica preso ao Hub e à versão da lista em que foi aberto: se a conexão ou a lista mudarem antes de Salvar, nada é enviado e ele pede para abrir de novo. Se outro aparelho mudou a lista ou o Hub não respondeu, o app relê a lista e pede para conferir, sem repetir sozinho.
  - Criar e mudar spaces exige a conexão local com o Hub, como a deste Mac ou por túnel.
  - Ainda não dá para enviar mensagens nas conversas de um space, criar, renomear, fechar ou reabrir conversas, nem apagar um space: o app diz isso no lugar da caixa de mensagem, sem botão desabilitado.

## Barra lateral

- Aberta (padrão, 232 px, arraste a borda de 200 a 320 px) ou **recolhida**: só a faixa, e a conversa ocupa a largura toda. Arrastar a borda bem para dentro também recolhe.
- Recolhida, encostar o ponteiro na borda da faixa traz a barra flutuando por cima da conversa, sem empurrá-la. Ela some quando o ponteiro **e** o foco saem (digitando em Buscar bots ela fica). `Esc`, de onde estiver o foco, a esconde depois de fechar menus, paleta ou painéis abertos, e o foco volta ao botão da faixa.
- Com a janela abaixo de 900 px, ela recolhe sozinha; o botão da faixa (ou `⌃⌘S`) a mostra e esconde flutuando.
- **Recolher/mostrar barra lateral**: botão no cartão de abas do topo da conversa, à esquerda da aba do bot (também `⌃⌘S`). Com a barra flutuando por cima, ele fica logo à direita dela.
- **Nova sessão (+)**: no mesmo cartão, logo depois da aba do bot, também com as sessões recolhidas. Cria uma sessão vazia daquele bot, abre a aba dela e põe o cursor na caixa de mensagem; se a criação falhar, o aviso aparece sobre a caixa e o foco volta ao **+**. Com a barra lateral flutuando por cima, o **+** sai (ficaria embaixo dela) e volta quando a barra sai. Só aparece no chat de um bot do Pessoal com um Hub que cria sessões (veja [Sessões por bot](context-and-memory.md#sessões-por-bot)); some em canais e Spaces.
- Recolher e abrir a barra animam a largura dela (0,3 s, a mola de layout do sistema), como o arrasto da borda: a conversa, o cartão de abas e o botão acompanham. Com movimento reduzido, a troca é seca.
- Ao voltar de Rotinas, o foco volta ao botão Rotinas da faixa.
- Ao voltar de Ajustes, o foco volta a quem abriu (o botão da chamada, por exemplo), senão à engrenagem da faixa.
- A antiga coluna estreita só de ícones saiu: Ajustes → Aparência → **Recolher barra lateral** usa a mesma preferência.

## Dentro da barra lateral

- No topo: o tipo do space (Pessoal), o nome no meio e o menu **+**, com Novo bot (`⌘N`), Novo canal e **Biblioteca de especialistas** (abre a biblioteca, sem criar bot).
- A **principal** (Berga) logo abaixo, no meio e sem caixa: rosto com a estrela de principal, ponto de estado no nome, o estado por extenso e o contador de não lidas. O menu `…` continua com Marcar como lida, Configurações e **Recomeçar a principal…** (o recomeço legado, sem resumo); com um Hub que cria sessões, também **Nova sessão** (veja [Sessões por bot](context-and-memory.md#sessões-por-bot)).
- **Buscar bots** logo abaixo da principal (`⌘K` também abre a paleta).
- **Canais** (as conversas com vários bots, com `#` e em minúsculas) e **Bots** (as conversas diretas, com os Times salvos no quadro e Sem time dentro), cada um com título que recolhe, contagem e **+**. A ordem e quais aparecem ficam em Ajustes → **Barra lateral** (arraste pela alça, `⌥↑`/`⌥↓` ou as setas; o interruptor esconde a seção sem apagar nada).
- Cada linha tem uma altura só: rosto, nome, a **função** do bot como etiqueta e, à direita, o que pede atenção (pedido de aprovação, erro, não lidas, trabalhando) ou a hora da última mensagem. O texto que antes ficava na segunda linha (atividade ou última mensagem) segue na dica e no leitor de tela.
- Canais conservam suas ações e arraste só entre canais. Bots usam o Time salvo; o menu Organizar no quadro abre o Pessoal do computador do item, mesmo com outro Space aberto.
- O **+** de Bots cria **Novo bot**. Nova seção e Mover para seção saíram da barra; os dados de seções antigas permanecem preservados.
- Com mais de um computador, o **+** de Canais ou Bots cria naquele computador. `⌥↑`/`⌥↓` só reordena o que está à vista: um bloco recolhido ou oculto mantém a ordem.
- Dentro de Bots, cada Time apresenta Chefe explícito elegível primeiro e membros; Sem time vem no fim. Recolhimento e ordem são locais, sem mudar associação. Busca revela resultados sem gravar fold; sem leitura válida há lista simples. Veja [Equipes na barra lateral](times.md#equipes-na-barra-lateral). Isso não cria conversa de Time.

## Conversa

- No topo, só o rosto de com quem você fala (a principal maior, com a estrela) e o nome numa pílula; clicar abre os detalhes. O botão redondo **(i)** no alto à direita também abre e fecha os detalhes, pelo clique ou pelo teclado. Sem fundo escolhido, nada muda de cor; com fundo, a conversa fica direto sobre ele e todo texto mora num balão, numa pílula ou num cartão de vidro.
- A conversa some por baixo do topo e da caixa de mensagem por máscara, sem faixa pintada.
- **Linha do tempo** na margem esquerda: um risco por mensagem carregada (vizinhas se juntam quando são muitas). O risco da mensagem à vista é mais longo; não lidas ficam na cor de destaque. Passar o ponteiro mostra hora e prévia; clicar (ou focar e apertar Enter, com as setas para andar) leva à mensagem.
- Balões: bot em vidro claro com **nome e hora dentro**, pessoa em vidro mais denso; o rosto do bot ao lado do balão no começo de cada sequência. O aviso de execução de rotina aparece como "Rotina · <nome>" e abre a rotina. Separadores de dia ("Hoje 09:12"), avisos e "Carregar mensagens anteriores" ficam em pílulas.
- Caixa de mensagem: cresce até **10 linhas** e depois rola por dentro, com os botões presos embaixo. O texto de ajuda diz "Mensagem para Byte" ou "Mensagem para #canal" ("Deixe uma mensagem na fila…" enquanto o bot trabalha). Vazia, o botão principal é **Ao vivo** (conversa por voz, quando disponível); com texto, vira Enviar. O **escudo** (só em conversa direta) abre o menu de aprovação: o nível em uso com visto (**Pedir liberação** ou **Liberar**, que também mostra os pedidos do motor em cartão), **Auto aprovar** e **Acesso total** como Em breve, e o caminho para **Configurações de aprovação**. Nada muda pela caixa: o nível só muda nas configurações do bot, com as checagens da própria tela.

## Cartões do chat

- **Aprovação:** o pedido mostra "Esperando você", o que o bot quer, o comando ou arquivo, os fatos (ação, onde roda, quem decide) e as opções que o Hub mandou. **Permitir** se segura por 0,9 s (ponteiro, Espaço ou Enter); soltar antes cancela. A permissão mais ampla enche com um tom suave da cor de destaque e o texto continua legível durante todo o gesto. Com **Reduzir movimento** um toque basta, e a ativação por tecnologia assistiva é imediata. **Recusar** é um clique. As opções, os ids e a resposta são os mesmos de antes: nada é aprovado sozinho nem ganha escopo novo. Depois da resposta, o selo diz o resultado (Permitido uma vez, Recusado, Expirou…). O Hub não manda prazo nem desfazer, então não há barra de tempo nem Desfazer.
- **Passos:** sob a resposta, um resumo que abre ("2 passos · Comando, Edição · 41 s") com cada passo e seu estado; enquanto o bot trabalha, a lista aparece aberta embaixo da linha de atividade, com o passo em andamento girando (parado com Reduzir movimento). Tudo vem das entradas de ferramenta que o Hub já registra; sem entradas, não aparece nada. Texto parcial conserva a lista ao vivo; só a resposta final recolhe os passos. Ferramentas de turnos anteriores ficam fora da atividade de um pedido novo.
- **Tabelas:** a tabela que o bot manda em Markdown vira uma tabela com busca em todas as linhas, ordenar por coluna (números alinhados à direita), quebrar linhas (ligado), copiar para planilha (TSV), baixar CSV pelo diálogo de salvar do app e expandir (Esc volta). `<br>` dentro de uma célula vira quebra de linha; qualquer outra marcação aparece como texto. A cópia TSV e o CSV protegem cabeçalhos e dados que podem ser interpretados como fórmula; só literais decimais estritos preservam o sinal negativo sem proteção adicional. Isso não muda a ordenação numérica da tabela.
- Ainda não: perguntas com opções A–E (o Hub não manda esse tipo de pergunta) e recibo "leu agora" (o Hub não manda leitura).

## Ajustes (tela, não mais folha)

- **Ajustes** abre como uma tela, no mesmo lugar e do mesmo jeito que Rotinas: ocupa a barra lateral e a conversa, a faixa continua à vista e o fundo continua atrás. O que estava aberto (conversa com rascunho, editor do bot, space, chamada) fica montado por baixo, escondido e sem foco, e volta como estava.
- Abre pela engrenagem (Ajustes, Uso), por **Conta e suporte** (Uso, Ajustes), por **⌘,** ("Ajustes…" no menu do app), pelo botão de ajustes da chamada (Voz), por Gerenciar computadores e pelo atalho de Computadores. Cada um abre na página certa.
- À esquerda, a lista de páginas (Pessoal: Geral, Aparência, Barra lateral, Contas de IA, Voz, Uso; Este Mac: Computadores, Ícone do app, só informativo). À direita, a página, com o título no alto; só a página rola, então nada fica cortado no topo e nada aparece atrás. Os dois painéis são cartões opacos; com fundo, a placa é vidro, sem desfoque com a transparência reduzida.
- **Abas** só nas páginas longas: **Contas de IA** tem Contas e Motores; **Voz** tem Voz para texto, Ao vivo e Chaves. O botão de ajustes da chamada abre Voz na aba Ao vivo. A tela lembra a última aba de cada página enquanto o app está aberto.
- **Voltar** (no alto) e **Esc** voltam ao que estava antes. Com uma folha ou diálogo aberto dentro de Ajustes (por exemplo, Adicionar computador por SSH), o primeiro Esc fecha só a folha.
- Ir a um space, ao Pessoal, a Rotinas, a Times ou abrir um bot por uma notificação fecha Ajustes e vai direto ao destino; abrir Ajustes fecha Rotinas e Times (uma tela por vez). Com Ajustes aberto, a engrenagem fica marcada e o Pessoal perde a barrinha de lugar atual; ⌘N, ⌘F e ⌘K esperam.
- O menu da conta (Conta e suporte) abre por cima de Ajustes, Rotinas e Times.
- Funciona na janela mínima (760×500) e com a barra lateral recolhida.

## Extensões (tela, não mais folha)

- **Extensões** abre como tela, igual a Ajustes: ocupa a barra lateral e a conversa, a faixa continua à vista, e o que estava aberto fica montado por baixo, escondido e sem foco.
- Páginas: **Conectores** (catálogo e Minhas contas; Aplicativos não existe mais) e **Skills**. **Conectores** na faixa abre Conectores; **Conectores** e **Skills** na engrenagem abrem as suas. Em Conectores a faixa marca Conectores; em Skills, a engrenagem.
- **Voltar** e **Esc** voltam ao que estava antes; uma folha aberta dentro (Adicionar MCP, um diálogo) fecha antes da tela.
- Uma tela por vez: abrir Extensões fecha Rotinas, Times e Ajustes, e abrir um deles, ir a um space, ao Pessoal ou abrir um bot por notificação fecha Extensões. Com Extensões aberta, ⌘N, ⌘F e ⌘K esperam, como em Ajustes.

## Janela baixa (menos de 600 px de altura)

- Na janela mínima (760×500) ou em qualquer altura abaixo de 600 px, o cartão de aprovação guarda os fatos (Ação, Onde, Quem decide) em **Detalhes**; as opções e o gesto de segurar não mudam.
- Em Times, o painel de detalhes vira uma gaveta: o botão (i) **Detalhes** no canto do quadro abre e fecha, e Esc também fecha.
- Em Rotinas, o aviso da pausa vira uma linha, com **Saiba mais** para a explicação inteira.
- De 600 px de altura para cima, nada muda.

## Rotinas e aparência dos bots

- **Rotinas** (botão da faixa) ocupa o lugar da barra lateral e da conversa, com a faixa ainda à vista e o mesmo fundo atrás; a conversa continua montada por baixo (rascunho, seleção e rolagem ficam), escondida e sem foco enquanto Rotinas está aberta. Voltar devolve o foco ao botão Rotinas. Enquanto Rotinas está aberta, a barrinha de lugar atual sai do Pessoal e vai para o botão Rotinas da faixa (o leitor de tela anuncia "página atual"); ao voltar, ou ao abrir um bot de fora, ela volta para o Pessoal.
- A tela tem a barra (Voltar, título, busca, filtros, Lista/Calendário e Nova) e dois painéis: à esquerda o minimês, os bots e a dica; à direita o calendário, com a barra de período (‹ ›, Hoje, datas e Dia/Semana/Mês) dentro dele. Na semana: hoje e a linha de agora no destaque, horas de 44 px, cada rotina na cor do rosto do bot (26% sobre o cartão) com o rosto ao lado do nome; numa meia hora aparece só o nome, numa linha, mesmo quando duas rotinas dividem o horário (hora e bot ficam na dica, no leitor de tela e na Lista). Nenhum texto da tela fica abaixo de 11px. Os dois painéis são cartões opacos; com fundo, a página é vidro. Arrastar um bot para um horário continua criando a rotina como antes.
- O editor de avatar usa as cores do sistema (cartão, grupos, campos, destaque).
- Ainda não: trocar o ícone do app (Dock). O app não tem como trocar o próprio ícone em uso; a galeria de variações fica para quando existir essa capacidade no processo principal. A marca oficial não muda.

## Command + Space para o Berga

No Mac, Ajustes → Geral oferece **Command + Space abre o chat com o Berga**,
desligado por padrão. Só atua com a janela em foco, fora dos campos de edição,
terminais e composição IME. Menus, diálogos e edição de bot conservam o teclado.
Abre ou foca a conversa principal do Berga do Space ativo. Se ela não estiver
disponível, mostra aviso e mantém o Space, sem abrir o Pessoal. No Pessoal,
seleciona a aba principal do Berga do Hub ativo, preservando rascunhos de extras.

O macOS reserva esse atalho para o Spotlight. Ao ligar, a opção explica como
desativá-lo em **Ajustes do Sistema → Teclado → Atalhos de Teclado → Spotlight**
e oferece um botão para abrir o painel. A Mexerica não altera o sistema nem
registra um atalho global. Para desligar, basta voltar à mesma opção.
