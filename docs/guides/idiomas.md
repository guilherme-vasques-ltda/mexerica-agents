# Idiomas do app

O app do Mac tem seis idiomas: **português do Brasil (pt-BR)**, **inglês (en)**, **espanhol (es)**,
**francês (fr)**, **chinês simplificado (zh-CN)** e **árabe (ar-AE, da direita para a esquerda)**. O pt-BR é a
fonte dos catálogos (toda chave nasce nele).

**Idioma padrão: inglês.** Sem escolha salva (`appLanguage` nulo), o app fala inglês; quem escolheu um
idioma continua nele. Um texto sem tradução cai primeiro no inglês e,
sem inglês, na fonte pt-BR (`CADEIA_FALLBACK` em `lib/fork-i18n/index.ts`, também nos plurais). Durante a transição as
telas ficam híbridas: as telas já traduzidas aparecem em inglês, e as legadas continuam em
português até a tradução chegar. O `<body>` segue com `lang="pt-BR"` (`LIMITE_LEGADO`) até o inventário do legado
zerar, porque é isso que os textos legados são de fato. A tradução do app inteiro chega por lotes; enquanto isso, fora do pt-BR, o app
avisa que a entrada na conta e a maior parte das telas ainda estão em português. Os bots continuam respondendo
em português, e isso também é avisado; o Berga segue o idioma do app, como abaixo. Nada
  disso muda o idioma do Hub nem dos outros bots.

## Para quem usa

- **Primeira abertura:** o primeiro passo é **Passo 1 de 2 — Escolha seu idioma**, com o idioma do
  sistema já escolhido quando ele é um dos 6 ("Sugerido pelo idioma do seu Mac"); senão, English. A sugestão só
  pré-seleciona; a pessoa confirma. A roda e Ajustes → Geral mostram English primeiro, Português (Brasil) em segundo
  e depois os outros. ↑/↓ trocam, Home/End vão às pontas, Enter ou
  **Continuar** seguem para a apresentação animada, já no idioma escolhido, e depois para a conta (ainda em
  pt-BR). Com reduzir movimento, Continuar leva direto à conta. A tela inteira muda na hora; não precisa reiniciar.
- O tema do topo (claro/escuro) grava a mesma escolha de Ajustes → Aparência.
- **Depois:** Ajustes → Geral → **Idioma**. A troca vale na hora, na janela inteira.
- O passo de idioma aparece uma vez por computador. "Apagar todos os dados" o traz de volta.
- **Sem escolha salva, o Berga responde em inglês.** O app manda o idioma
  efetivo ao Hub (`donnaAppLanguageSave`), e sem escolha esse idioma é `en`. Como o Berga segue o idioma do app,
  o Berga passa a responder em inglês até a pessoa escolher outro idioma. O Hub não
  mudou.
- **O Berga responde no idioma do app.** O idioma escolhido vai ao Hub ao conectar e a cada troca
  (`store/fork-idioma-hub.ts`); o Berga responde nele. Os outros bots seguem como antes. Contrato em
  [Berga fixo](../features/donna-fixa.md).

## Estado desta base (o que ainda não foi migrado)

- Traduzidos nos seis idiomas: o passo de idioma inteiro (inclusive nomes acessíveis), a seção Idioma dos Ajustes, a linha de troca de modelo no chat com o balão técnico (`conversa.troca.*`) e as notas da voz transcrita no mini WhatsApp e o interruptor "Transcrever áudios recebidos" dos Ajustes do bot, que diz para onde vai o áudio (`whatsapp.voz.*`; o resto do mini WhatsApp segue na etapa dele).
- Na entrada, a apresentação animada (região, descrição acessível, Pular e rótulos Financeiro, Marketing e
  Social; Stripe, Instagram, YouTube e a assinatura da marca são nomes próprios, iguais em todo idioma) e a espera
  **Verificando conta salva…**. Para a escolha valer na apresentação (que só aparece uma vez), o passo de idioma
  vem antes dela: idioma é o primeiro passo.
- Também nos seis idiomas: a folha **Novo bot** (assistente de criar bot, chaves `botCreate.*` e rótulos das funções
  `botRoles.<id>.*`). Na folha ainda ficam em português os textos de componentes compartilhados (editor de avatar, linha
  da barra lateral, diálogo de confirmação) e as mensagens de validação da função, que entram nos lotes deles.
- **Conversa:** nos seis idiomas, a conversa aberta: campo de mensagem e anexos, balões e seus
  estados (enviando, na fila, não enviada), reações e Mais ações, cartão de aprovação e menu do escudo, avisos
  (passagem, memória, equipe, rotina), mensagens entre bots, Conversa completa, tabela, passos do bot, cartão de
  pergunta, gráfico, recibo do turno e as propostas de skill. Cada cartão declara `lang`/`dir` próprios. Ainda em
  pt-BR dentro da conversa: os textos que vêm prontos de outros módulos (natureza e origem da memória, estado da
  pergunta ao dono, nomes de motor de `textos-pt`), a ilha, a chamada de voz e o que o Hub manda pronto.
- Também nos seis idiomas (chaves `ajustes.*`): a tela de Ajustes (cabeçalho, lista de páginas e abas),
  Geral (Sistema, Autonomia geral, Privacidade), Aparência (tema, destaque, fonte, fundos, transparência), Barra
  lateral, Uso, Atualizações, Computadores e dispositivos (inclusive SSH e dispositivos autorizados), Acesso ao
  computador, a folha Pedido de acesso, o filtro de computadores da barra lateral e a conversa por voz dos Ajustes. O
  main (`main/ssh.ts`) não sabe o idioma escolhido: manda cada passo e falha do SSH com o motivo (`SSHMotivo`: chave e
  parâmetros) e o mesmo texto em inglês, e a tela mostra o motivo no idioma dela (`shared/ssh-startup.ts`). Listas de
  dados (temas, cores, fundos, seções, transparência, fontes) levam a chave (`labelKey`/`nameKey`) e o `label` pt-BR
  lido do catálogo, para quem ainda lê o rótulo direto. Ficam para os lotes deles: Contas de IA, Voz (`fork/voz`) e Bot
  de fundo.
- **Navegação:** nos seis idiomas, a janela principal (estados vazios, sem conexão, menus de bot,
  confirmações), a barra lateral (bots, seções, cartão do principal, busca), a faixa de spaces, o menu da conta, a
  busca rápida, os avisos do Hub e de atualização, a ilha, o pareamento e a conta (passo 2: entrar, criar conta,
  código, segundo fator, nova senha, lista de espera, Apagar todos os dados). Ficam para os lotes deles
  a bandeja e o menu do Mac (`main/tray.ts`, `main/index.ts`), spaces e equipes da barra.
- **Extensões (chaves `extensoes.*`):** a tela de Extensões (páginas Aplicativos, Conectores e
  Skills), o Marketplace (Meus MCP, instalados e remover), Adicionar MCP, Credenciais (cofre, 1Password e logins), o cartão de pedido de
  conexão na conversa, a folha do motor no Hub e o terminal de instalação. Exemplos de código nos campos (URL, comando, config, cabeçalho) são iguais em todo
  idioma e da esquerda para a direita. A página Skills e a lista de skills instaladas são descritas abaixo, no
  idioma escolhido dentro da tela.
- **Voz e chamada:** nos seis idiomas, Ajustes → Voz (Voz para texto, Ao vivo, chaves deste
  aparelho e do Hub, com os resultados de Guardar, Testar e Limpar) e a chamada (barra, limites, avisos e falhas).
  "Neste Mac/neste computador" e "da OpenAI/do Gemini" são expressões do catálogo (`voz.ajustes.aparelho.*`,
  `voz.ajustes.provedor.*`) que entram nas frases inteiras como parâmetro ao desenhar (`fork/voz/frases.ts`). A falha
  da chamada e os resultados das chaves ficam guardados como dado (`Mensagem`). Ficam em português, de
  propósito, as instruções e as respostas de ferramenta que só o modelo de voz lê (`call-operator.ts`,
  `shared/fork-voz-operador.ts`, `operador-voz.ts`, `encerrar.ts`, `tempo-real-ws.ts`, `voice-approval.ts`,
  `voice-call.ts`) e o `main/fork-voz/cofre.ts`.
- **Bots:** nos seis idiomas, as Configurações do bot (autonomia, pasta, ferramentas, aparência,
  roteamento, índice de skills e as mensagens de erro do editor e das skills), o painel do bot e do canal, o editor
  de canal e o novo canal. Os erros do editor e das skills saem no idioma do `<html>` (como `lib/fork-donna-erros`).
  A lista da barra lateral (`BotRoster`, `BotRow`, seções) e o cartão do bot principal estão na navegação.
- **Times e Spaces (chaves `times.quadro.*` e `spaces.*`):** o quadro de Equipes (Quadro e Lista,
  detalhes, Reporta a, Mover, rotas, áreas, modelo Marketing), a lista de equipes na barra (sala da equipe, grupos de
  Agentes) e Spaces (o space aberto, criar e editar space, as recusas do Hub). A sessão de Equipes, as regras da faixa
  de spaces e o contexto do space guardam `Frase`, traduzida ao desenhar; nomes de bot, área e space e o que o
  Hub manda pronto entram como dado (`cru`), isolados. Horas em LTR. O conteúdo das folhas abertas por `Sheet` (portal
  no body) declara o idioma. Ficam pendentes em `src/shared/`: `shared/fork-times.ts` (recusas locais do quadro,
  que aparecem como vieram) e `SCREEN_USE_UNAVAILABLE_TEXT`; a Torre é traduzida à parte. As regras do modelo Marketing que vão às
  instruções dos bots criados continuam em pt-BR (`lang="pt-BR"` na folha), até a decisão do dono.
- **Catálogo de skills e cartão de conectores (chaves `skills.*` e `conectores.*`):** a página Skills
  (Base central, Instaladas, Catálogo), os detalhes de uma skill, escrever à mão, comparar rascunho, a folha da skill
  nos ajustes do bot e o cartão de aprovação de ação de conector. A loja do catálogo (`catalogo-store.ts`), as regras
  e a matriz da Base central guardam `Frase` (chave + parâmetros, `lib/fork-i18n/frase.ts`), nunca o texto pronto: a
  falha, o aviso e o resultado incerto saem no idioma da tela mesmo depois de trocar. O diff da comparação é código
  (LTR isolado); o cartão de aprovação declara `lang`/`dir` e mostra conta de saída (LTR) e campos (`dir="auto"`)
  isolados. Mensagens de protocolo que o app compara com a resposta do Hub (`shared/fork-skills.ts`, `revisao.ts`)
  não se traduzem (`textos-ok`).
- **WhatsApp:** nos seis idiomas, Configurações do bot → WhatsApp (estados, opções, cuidados contra
  banimento, número, desconectar), Ajustes → Plugins (card do WhatsApp e o wacli), Conectar número (riscos, QR, código
  e resultado), o mini WhatsApp do painel e a tela grande. Erros do Hub e de leitura ficam guardados como dado
  (`Mensagem`) e são traduzidos ao desenhar; datas e horas da caixa seguem o idioma do app. O texto das mensagens
  e as frases prontas do Hub ficam como chegaram. Ajustes → Voz e a chamada têm seção própria acima.
- **Memória:** nos seis idiomas, a Memória do bot (Sobre você, Este bot, Não salvei, Consolidações
  e a aba do motor), as lembranças e o menu de cada uma, as notas e suas versões, a folha da memória própria do motor,
  e os selos de natureza, origem e versão, inclusive nos recibos da memória automática na conversa. Autor e versão de
  nota são frases inteiras por autor e causa (`memoria.painel.versao.<autor>.<causa>`), com o nome do bot isolado.
  O erro da memória fica guardado como dado (`Mensagem`). O texto das lembranças e notas é da pessoa ou do bot e
  não é traduzido. Fica em português, de propósito, o conteúdo de exemplo do demo (`memoria-auto/demo.ts`).
- **Barra e menus do Mac, e textos de `shared/`:** nos seis idiomas, o menu da barra (estado, ações,
  Ajustes, Atualizações, tela remota, uso), o menu Visualizar e a janela de parear (`sistema.barra.*`), o que a janela
  manda à barra (`views/tray-summary.ts`), "Aguardando" com a hora no formato do idioma (`sistema.conversa.espera.*`), "Parou pelo limite do plano; volta às HH:MM" e a hora em que cada limite do plano volta, em hora local relativa (hoje, amanhã, dia da semana ou data; `sistema.barra.uso.volta.*`, `lib/limite-volta.ts`),
  o modo rápido (`sistema.conversa.rapido.*`) e as recusas e erros de Equipes (`times.regras.*`, que `shared/fork-times.ts`
  devolve como `Frase`). O erro do cofre de voz é só a categoria. Os erros que o processo main lança como texto ainda ficam
  em português.
- **Ainda em português:** os erros que o processo principal manda prontos na entrada (recuperação da
  conta, `fork/account`), o resto dos Ajustes
  e todas as outras telas, menus e mensagens do Hub. Por isso o indicador mostra **2 passos**; o passo 3
  (primeiro nome e cartão de prontidão) entra depois e o indicador muda junto.
- O aviso de migração sai quando o app inteiro estiver traduzido. O aviso dos bots fica enquanto eles
  responderem em português.
- **Mission Board, Bot de fundo e Onde roda:** nos seis idiomas, o Mission Board inteiro (cartões,
  seções, detalhe, responder pelo Board, pedidos entre bots e "O que aconteceu"), Ajustes → Bot de fundo (funções,
  espaços e histórico) e Ajustes → Onde roda (cadastro do servidor, chaves públicas, guia e verificação). Os rótulos de
  `shared/fork-torre*` viraram dado (código, número, dia relativo) e a tela traduz ao desenhar; o rótulo fixo que o Hub
  manda para "Abrir conversa do bot" é ignorado e vem do catálogo. Erros e avisos do Board, do Bot de fundo e dos
  lugares ficam como código no estado. Horas e "há quanto tempo" saem pelo `Intl` do idioma (horas isoladas
  LTR); nomes de bot, time, space e Hub vão isolados; IDs, chaves, portas e endereços, da esquerda para a direita.
  Continua como o Hub manda: motivo informado pelo computador, nomes de função desconhecidos e o texto
  do resumo do dia e do guia de preparo.
- **Atividade do Hub (uma autorização só):** o app não lê nem traduz o texto de atividade que o Hub manda
  (`runtime.activity`, em inglês: "Typing…", "Needs approval: …"). Ele fica como veio até o Hub passar
  a mandar código em vez de frase. Nomes de membro e títulos são texto livre; reconhecer frases dentro deles não tem
  como ser seguro. Os textos em português vêm do estado estruturado: aprovação aberta (`status` `needsInput` e cartão
  pendente) dá "Precisa da sua aprovação" e, com o cartão na tela, "Esperando a sua decisão acima"; o cartão
  respondido dá "Você permitiu" / "Você recusou" (`lib/principal.ts`, `views/thread/decisao.ts`). Única exceção,
  a da main: no balão, `views/fork/donna/DonnabotThinkingMark.ts` traduz as frases fixas ("Thinking…", "Writing a
  reply…", "Starting agent…"… e "Typing…" → "Escrevendo…") só por igualdade com a string inteira da atividade; com
  qualquer prefixo ("Berga: Thinking…"), fica como veio. Na barra, fila, painel e Torre, o texto do Hub aparece cru.
- **Contas de IA:** nos seis idiomas, Configurações → Contas de IA: a lista e o detalhe das contas,
  adicionar conta, conectar e reconectar (etapas, código da página, terminal de setup), chave de API, uso, os motores
  do Hub e as mensagens de erro e de falha por código. `contas-ia/textos.ts` virou `textosEm(idioma)` (puro, usado pelas
  regras e pelos testes) e as telas usam `useTextosContas()`. Os erros ficam crus no store e na sessão de setup e são
  traduzidos ao desenhar (`errorMessage(erro, idioma)`); código desconhecido mostra a mensagem do Hub como veio. Nomes de
  bot na lista vão isolados (FSI…PDI) e a lista sai do `Intl.ListFormat`. Continuam como o Hub manda: o nome da conta,
  dos bots e dos motores, e as mensagens de erro sem código conhecido.
- **Conta, Cloud e aparelho:** nos seis idiomas, o perfil e a saída da conta (Ajustes e boas-vindas),
  o registro deste aparelho e Ajustes → Computadores → Pela nuvem (registrar, Hubs da conta, vincular pelo SSH, código
  de conferência, primeiro vínculo de um Hub novo e resultado incerto). Frases com "este Mac"/"este computador" são
  inteiras no catálogo (`….mac`/`….pc`), nunca montadas em pedaços. O aviso do Cloud fica como código no modelo
  (`notice`) e vira frase ao desenhar. O nome do Hub vai isolado (FSI…PDI, `<bdi>` nas linhas); a hora do prazo, o
  código de 6 dígitos e o identificador do pedido, da esquerda para a direita.
- **Rotinas:** nos seis idiomas, a tela central Rotinas (lista, calendário, barra lateral,
  pausa do Hub), o painel de rotinas do bot, o editor (agenda, webhook, fuso, tempo limite). O cartão Agenda de Ajustes →
  Plugins e seu catálogo exclusivo foram retirados. Datas, dias da semana e meses saem do `Intl` no idioma escolhido (`lib/routine-calendar.ts` recebe o
  idioma). O editor deixou o mapa `editor-pt.ts` (texto em inglês do upstream → pt-BR, sem fallback) e chama as
  chaves `rotinas.editor.*`. Continuam como o Hub manda: a agenda por extenso da lista e do calendário (pt-BR,
  `calendar.rs`); no editor, fora do pt-BR, aparece o `summary` em inglês do upstream no lugar do `resumo` pt-BR
  (`agendaDoHub`). Mensagens de erro do Hub e o nome das seções e dos bots também vêm como estão.
  O que abre em portal no body (seletor de fuso, folha de escolher bot, confirmações de apagar rotina e de trocar a
  chave) declara `lang`/`dir` da tela; o `Dialog` recebe `language` (`PortalLanguage`) para
  isso, e cada confirmação migrada passa `cancel={t(…)}` (o padrão "Cancelar" do Overlay continua em português).
  Nome e instrução da rotina usam `dir="auto"`; nome e busca dentro de frase vão entre FSI e PDI. Pendente: os menus do
  `ChoicePicker` (filtros de seção e de bot, minuto) abrem no body sem o idioma da tela até a prop `language` ser passada.

## Arquitetura

| Parte | Onde | O que faz |
| --- | --- | --- |
| Idiomas e sugestão | `apps/desktop/src/shared/fork-idioma.ts` | `AppLanguage` (os seis), `direcaoDe` (rtl só em ar-AE), `sugerirIdioma(lista)`: o primeiro `pt-*`, `en-*`, `es-*`, `fr-*`, `ar-*` ou chinês simplificado (`zh`, `zh-Hans`, `zh-CN`, `zh-SG`) da lista; chinês tradicional (`zh-Hant`, TW, HK, MO) não recebe sugestão. |
| Leitura do sistema | `src/main/fork-idioma.ts` + 1 linha em `main/index.ts` | Lê `app.getPreferredSystemLanguages()` localmente. Sem rede, IP, localização ou permissão. |
| Ponte | `window.codync.suggestedLanguage` (`preload/index.ts`, `shared/ipc.ts`) | Só o valor sugerido (um `AppLanguage` ou `null`), nunca a lista do sistema. A demo usa `?idioma=` ou `navigator.languages`. |
| Catálogos | `src/renderer/lib/fork-i18n/<idioma>/` (pt-BR é a fonte; `en`, `es`, `fr`, `zh-CN`, `ar-AE`), um módulo por área, e `registro.ts` | Chaves completas por domínio (`idioma.passo.titulo1`), estáveis. Mapa e regras em [Catálogos por área](#catálogos-por-área). O tipo `Catalogo` exige todas as chaves comuns e o `other` de cada plural; `Traducao` faz o mesmo em cada módulo. Os caminhos antigos `pt-BR.ts`, `en.ts`… só reexportam (`ptBR`, `en`, `es`, `fr`, `zhCN`, `arAE`). |
| Tradução | `src/renderer/lib/fork-i18n/index.ts` | `traduzir(idioma, chave, params?)`, `traduzirPlural`/`pluralEm`, `categoriasPlural`, `formatarData/Numero/Moeda`; texto ausente segue `CADEIA_FALLBACK` (idioma → en → pt-BR). Sem DOM. |
| Processo principal | `src/main/fork-idioma.ts`, `src/renderer/store/fork-idioma-main.ts` | O renderer manda o idioma efetivo (`app:setLanguage`) ao abrir e a cada troca; o main começa em inglês. O menu do Mac (`shared/fork-menu-app.ts`), o painel Sobre, o menu Visualizar e a barra de menus seguem esse idioma e se refazem na troca. Cada rótulo de `main/tray.ts` e `main/index.ts` chama o catálogo (`sistema.barra.*`) com `idiomaDoApp()`, nos seis idiomas; a tabela pt-BR de `shared/fork-textos-barra.ts` saiu, e fica um mecanismo só. |
| Estado | `src/renderer/lib/fork-i18n/estado.ts` | Preferência `appLanguage` (mesmo `pref` de `lib/prefs.ts`), `useIdioma()`, `useT()` (com `t.plural`), `idiomaAtual()`, `superficieIdioma(idioma)`; sincroniza `lang` e `dir` do `<html>` e o limite do legado no `<body>`. Sem escolha: inglês; com escolha salva, ela. |
| Escritas e RTL | `src/renderer/views/fork/idioma/escritas.css` | `.idioma-superficie` (pilha de fontes por escrita), `.espelhar-rtl`, `.bidi-isolado`. |
| Passo 1 | `src/renderer/views/fork/idioma/` | `PassoIdioma`, `IdiomaAjuste`, `AvisosIdioma`, regras em `passo.ts`. Gancho em `AccountWelcomeView.tsx` e `SettingsView.tsx`. |

O passo de idioma só existe dentro das boas-vindas, com a conta não confirmada (`signed_out`). Ele não lê nem
altera `accountEntry`: a preferência de idioma nunca libera o portão, e nada do app ou do Hub monta antes de
`confirmed`.

### Catálogos por área

Cada idioma tem uma pasta com os mesmos módulos (`pt-BR/whatsapp/numero.ts`, `en/whatsapp/numero.ts`…). Cada
módulo exporta só `export const textos = { 'chave.completa': 'texto', … }` (no pt-BR com `as const`; nos outros,
`Traducao<typeof fonte>` do módulo pt-BR, que o typecheck usa para exigir as mesmas chaves). O `index.ts` da pasta
importa cada módulo e monta o catálogo com `...parte` de cada um, sem I/O nem glob. O nome do arquivo não muda a
chave: as chaves continuam completas.

A chave pertence ao módulo de **prefixo mais longo** do registro (`src/renderer/lib/fork-i18n/registro.ts`):

| Prefixo da chave | Arquivo em cada `<idioma>/` | Observação |
| --- | --- | --- |
| `times.` | `times.ts` | aviso do Berga fixo em Equipes |
| `times.quadro.` | `times/quadro.ts` | Equipes (quadro, lista, detalhes, seletores, áreas, modelo Marketing, barra) |
| `times.regras.` | `times/regras.ts` | regras do quadro de Equipes (`shared/fork-times.ts`) |
| `sistema.barra.` | `sistema/barra.ts` | barra e menus do Mac, Atualizações, tela remota |
| `sistema.conversa.` | `sistema/conversa.ts` | aguardando cota e modo rápido |
| `agenda.` | retirado | antigo cartão Agenda, interruptores, semana e autorização saíram do app nos seis idiomas |
| `rotinas.` | `rotinas.ts` | tela central Rotinas (`central`, lista, calendário, barra lateral), pausa do Hub (`pausa`), estados e desfecho das execuções (`estado`, `desfecho`), painel do bot (`painel`), editor (`editor`) e webhook (`webhook`) |
| `idioma.` | `idioma.ts` | |
| `marca.` | `marca.ts` | textos com o nome do app (Mexerica) ou do bot principal (Berga) que eram literais legados. Nas telas ainda não migradas saem em pt-BR (`traduzir('pt-BR', …)`), como o resto delas; o main e o shared também leem daqui |
| `navegacao.janela.` | `navegacao/janela.ts` | janela principal, menu da conta, busca rápida, Hub, atualização |
| `navegacao.barra.` | `navegacao/barra.ts` | barra lateral (bots, seções, principal, busca, faixa de spaces) |
| `navegacao.ilha.` | `navegacao/ilha.ts` | ilha (janela pequena no topo da tela) |
| `navegacao.parear.` | `navegacao/parear.ts` | pareamento do iPhone |
| `tema.` | `tema.ts` | |
| `ajustes.` | `ajustes.ts` | Ajustes gerais, Aparência, Barra lateral, Computadores, SSH, Acesso ao computador e conversa por voz |
| `aparencia.` | `aparencia.ts` | |
| `extensoes.` | `extensoes/geral.ts` | tela de Extensões, Marketplace, Adicionar MCP, Credenciais, pedido de conexão e motor no Hub |
| `skills.` | `skills/catalogo.ts` | catálogo de skills (página, detalhes, escrever, comparar, Base central) |
| `spaces.` | `spaces.ts` | Spaces (faixa, criar e editar space, space aberto) |
| `conectores.` | `conectores/cartao.ts` | cartão de aprovação de ação de conector |
| `contrato.` | `contrato.ts` | |
| `delegacao.` | `delegacao.ts` | |
| `grupos.` | `grupos.ts` | grupos de bots (menus, filtro de Rotinas, erros por `code`, contagem) |
| `memoria.painel.` | `memoria/painel.ts` | a Memória do bot (abas, lembranças, consolidações, notas, memória própria do motor) |
| `memoria.auto.` | `memoria/auto.ts` | memória automática (natureza, origem, versão, recibos e Não salvei) |
| `memoria.sugestoes.` | `memoria/sugestoes.ts` | Sugestões na Memória do bot |
| `memoria.` (o resto) | `memoria/ajustes.ts` | Ajustes → Memória |
| `whatsapp.numero.` | `whatsapp/numero.ts` | |
| `whatsapp.prazo.` | `whatsapp/prazo.ts` | |
| `whatsapp.conversa.` | `whatsapp/conversa.ts` | |
| `whatsapp.voz.` | `whatsapp/voz.ts` | reservado, vazio |
| `whatsapp.estados.` | `whatsapp/estados.ts` | os estados do plugin, do número e do envio, erros do Hub e desconectar |
| `whatsapp.pareamento.` | `whatsapp/pareamento.ts` | Conectar número (riscos, QR, código e resultado) |
| `whatsapp.ajustes.` | `whatsapp/ajustes.ts` | Configurações do bot → WhatsApp e Ajustes → Plugins |
| `whatsapp.caixa.` | `whatsapp/caixa.ts` | mini WhatsApp do painel e tela grande |
| `conversa.limites.` | `conversa/limites.ts` | |
| `conversa.arquivar.` | `conversa/arquivar.ts` | encerrar, continuar e Arquivar sozinho |
| `conversa.seletor.` | `conversa/seletor.ts` | |
| `conversa.troca.` | `conversa/troca.ts` | |
| `conversa.custo.` | `conversa/custo.ts` | custo estimado |
| `conversa.fila.` | `conversa/fila.ts` | bandeja da fila em cima do campo |
| `conversa.aprovacao.` | `conversa/aprovacao.ts` | cartão de aprovação, menu do escudo, botão de segurar |
| `conversa.compositor.` | `conversa/compositor.ts` | campo de mensagem, anexos, menu `/` |
| `conversa.mensagens.` | `conversa/mensagens.ts` | balões, estados de envio, reações, conversa entre bots, conexão, Conversa completa |
| `conversa.avisos.` | `conversa/avisos.ts` | passagem, aviso de memória, mudança de equipe, rotina, aviso do motor, pergunta ao dono |
| `conversa.cartoes.` | `conversa/cartoes.ts` | tabela, passos do bot, gráfico |
| `conversa.escolha.` | `conversa/escolha.ts` | cartão de pergunta com opções (genui) |
| `conversa.recibo.` | `conversa/recibo.ts` | recibo do turno |
| `conversa.skills.` | `conversa/skills.ts` | indicação, recomendação e rascunho de skill na conversa |
| `conta.` | `conta.ts` | perfil, saída e registro do aparelho da conta; frases por aparelho em `….mac`/`….pc` |
| `contas.` | `contas.ts` | Configurações → Contas de IA (lista, detalhe, nova conta, conectar, terminal de setup, chave de API), motores do Hub, erros por código (`contas.erro.*`) e motivos de falha (`contas.falha.*`) |
| `nuvem.` | `nuvem.ts` | Ajustes → Computadores → Pela nuvem (estados, vínculo, código, primeiro vínculo, avisos `nuvem.aviso.*`) |
| `torre.` | `torre.ts` | Mission Board (estados, seções, detalhe, respostas `torre.resp.*`, pedidos entre bots `torre.pedidos.*`, resumo do dia `torre.oqa.*`, erros `torre.erro.*`) |
| `fundo.` | `fundo.ts` | Ajustes → Bot de fundo (funções `fundo.funcao.*`, motivos `fundo.motivo.*`, espaços, histórico, avisos e erros por código) |
| `lugares.` | `lugares.ts` | Ajustes → Onde roda (cadastro, chaves públicas, guia, verificação `lugares.ver.*`, erros `lugares.erro.*`) |
| `instalacaoRemota.` | `instalacaoRemota.ts` | estados, motivos de incompatibilidade, bloqueios, eventos e mensagens da instalação remota do Host (só chaves; a tela fica à parte) |
| `contexto.custo.` | `contexto/custo.ts` | custo estimado |
| `contexto.` (o resto) | `contexto/medidor.ts` | anel, balão, compressão, Ajustes → Contexto |
| `entrada.` | `entrada.ts` | conta (`entrada.boasVindas`, `entrada.marca`) e restauração (`entrada.restaurando`). A apresentação (`entrada.intro.*`) saiu |
| `botCreate.` | `botCreate.ts` | reservado, vazio |
| `botRoles.` | `botRoles.ts` | reservado, vazio |
| `botSettings.` | `botSettings.ts` | reservado, vazio |
| `voz.ajustes.` | `voz/ajustes.ts` | Ajustes → Voz, chaves deste aparelho e do Hub, expressões de aparelho e provedor |
| `voz.chamada.` | `voz/chamada.ts` | a chamada (barra, limites, avisos e falhas) |
| `bots.editor.` | `bots/editor.ts` | editor antigo do bot, canal (novo e editar), modelo do bot, seletor de modelo |
| `bots.painel.` | `bots/painel.ts` | painel do bot e do canal ao lado da conversa (abas, Agora, membros, líder) |
| `bots.ajustes.` | `bots/ajustes.ts` | Configurações do bot (autonomia, pasta, ferramentas, aparência, roteamento, índice de skills, erros do editor e das skills) |
| `sessions.` | `sessions.ts` | reservado, vazio (Sessões) |
| `armazenamento.` | `armazenamento.ts` | reservado, vazio |

- **Chave nova numa área que já existe:** escreva-a no módulo da área nos seis idiomas. Índice e registro não mudam.
- **Área nova:** uma linha em `registro.ts` (`{ modulo, prefixo }`), o arquivo com o mesmo caminho nas seis pastas e
  a linha de import + `...parte` em cada `index.ts`; depois `npm test` (o gate abaixo). Registro coordenado, uma vez.
- Os arquivos planos antigos (`pt-BR.ts`…) servem só para imports existentes; texto acrescentado neles reprova.
- PRs que alteram a mesma chave continuam precisando de conciliação; a divisão evita só a disputa pelo fim do arquivo.

### Como usar num componente

```tsx
const t = useT()
<h2>{t('idioma.ajuste.secao')}</h2>
<p>{t('idioma.passo.etapa', { atual: 1, total: 2 })}</p>
```

- Parâmetros: `{nome}` no texto; o tipo exige exatamente os parâmetros do texto pt-BR.
- Plural (contagens inteiras não negativas): chaves por categoria CLDR, `x.zero|one|two|few|many|other`, e
  `t.plural('x', n, params?)`. Com `n === 0` e `x.zero` no catálogo, vale essa forma ("Nenhum bot"; opcional em qualquer idioma, exigida só em ar-AE); sem ela, a categoria vem de `Intl.PluralRules` do idioma; falta a forma → `x.other` do
  idioma → a mesma busca no inglês → no pt-BR (`CADEIA_FALLBACK`). `{n}` é reservado e entra formatado pelo idioma; uma forma pode omiti-lo (ex.: árabe "بوتان").
  Categorias obrigatórias por idioma = todas as que alguma contagem inteira não negativa alcança
  (`categoriasPlural`): ar-AE as seis; zh-CN só `other`; en `one` e `other`; pt-BR, es e fr `one`, `many` e
  `other` (`many` é a forma real dos milhões: "1 milhão de bots"). Nada inexistente é exigido (zh sem `one`) e
  nada existente fica de fora. Outros casos (decimais, ordinais): pedir na base, sem criar catálogo paralelo.
- Erros: a lógica e os stores emitem códigos estáveis; só a view converte em texto com `t()`.
  Estado que guarda um erro guarda a **chave** do catálogo (com parâmetros), nunca o texto já traduzido: trocar o
  idioma com o erro à vista troca o texto (`views/fork/bot-settings/mensagem.ts`).
- Datas, números e moeda: `formatarData(idioma, data, opções)`, `formatarNumero`, `formatarMoeda(idioma, valor,
  moeda)`. A moeda vem do dado (ISO 4217), nunca do idioma.
- Não traduza nomes e conteúdo do usuário, IDs, comandos ou credenciais.

### Direção (RTL) e escritas

- `<html lang dir>` segue o idioma escolhido. **Transição por superfície:** o `<body>` é o limite do legado,
  `lang="pt-BR" dir="ltr"`, porque as telas ainda em português (e os diálogos que abrem no body) não estão
  prontas para espelhar. Cada superfície migrada declara `{...superficieIdioma(idioma)}` (lang e dir próprios) e
  a classe `idioma-superficie`. Hoje: o passo de idioma, a seção Idioma dos Ajustes, as páginas de Ajustes traduzidas
  (lista e cabeçalho, Aparência, Barra lateral, Computadores, SSH, Acesso ao computador, Pedido de acesso e voz),
  a seção
  Limites de uso (Ajustes → Conversa) e os botões do aviso de espera do limite; a folha Novo bot, a restauração
  (`AccountWelcomeView`) e a própria conta (a entrada calma declara o idioma escolhido). A porta
  (`AccountEntryGate`) e `SignInMethods` não leem preferências: leem o idioma do `<html>` (`idiomaDoDocumento`) ou
  recebem os rótulos por prop. Quando o inventário do legado zerar, o limite do body sai e o app espelha inteiro.
  Nada de `transform` global para espelhar.
- CSS de superfície migrada: só propriedades lógicas (`inset-inline`, `margin-inline`, `padding-block`,
  `inline-size`, `text-align: start`).
- **Portais:** menus, diálogos e folhas (`Dialog`, `AnchoredMenu`, `DropdownMenu`, `ChoicePicker`, `Sheet`,
  `FormSheet`) abrem no `<body>`, que é o limite do
  legado. A tela migrada passa `language={superficieIdioma(idioma)}` para a raiz do portal e `cancel={t(...)}` em toda
  confirmação (o padrão do `Dialog` continua "Cancelar" em português).
  Isso inclui as folhas de Memória (editar lembrança, abrir nota e explicar a memória do motor) e de Projetos
  (criar ou editar projeto e tarefa), além do guia de preparo de Onde roda, mesmo quando o conteúdo interno já
  declara `lang` e `dir`.
- Espelham só ícones de direção (setas, chevrons de navegação), com `.espelhar-rtl`. Logo, check, play e texto
  do usuário não.
- **Código, comando, caminho e diff são sempre LTR e isolados** (regra de segurança): `dir="ltr"` no bloco e a
  classe `.codigo-ltr` (`unicode-bidi: isolate`, em `thread/permission-card.css`); trecho em linha com `<bdi dir="ltr">`.
  Num cartão RTL, sem isso o bidi mostra `+const x = 1;` como `;const x = 1+` e a pessoa aprovaria o que leu errado.
  Conteúdo da pessoa ou do bot dentro de superfície traduzida (campo de mensagem, citação, células, opções) usa `dir="auto"`.
  Teste: `views/thread/idioma-direcao.test.ts`.
- Em árabe, valores inseridos com `{parâmetro}` entram isolados (FSI…PDI). Nome, código ou número do usuário
  dentro de texto traduzido na view: `<bdi>` ou `dir="auto"` com `.bidi-isolado`.
- A mesma regra vale para host, e-mail e código de aprovação ou impressão de chave (Ajustes):
  - fora da conversa, o bloco usa `dir="ltr"` com `.bidi-isolado` (global, em `escritas.css`; `.codigo-ltr` é do
    `permission-card.css` da conversa);
  - em texto puro (título, aria, linha de status), `ltr()` de `shared/bidi.ts` (LRI…PDI); o nome do usuário vai com
    `isolado()` (FSI…PDI, também em idioma LTR);
  - comando e caminho não entram na frase do catálogo: vão como `{comando}`/`{caminho}`. O que a pessoa copia para o
    terminal fica num elemento próprio, sem caracteres isolantes;
  - campo técnico (host, usuário, porta) com `dir="ltr"`; campo e texto livres do usuário com `dir="auto"`.
- Trecho migrado dentro de tela que ainda tem legado declara `lang`/`dir` só nele (`superficieIdioma`, ou
  `Superficie` de `views/settings/rotulo.ts`, que não cria caixa); o resto fica no limite legado pt-BR/LTR.
  Menus e diálogos abrem num portal no body, que é legado: quando os itens ou o texto já são traduzidos, passe
  `language={superficieIdioma(idioma)}` ao `Dialog`, `AnchoredMenu` ou `DropdownMenu`, e o `cancel`
  pelo catálogo. Sem `language`, o portal fica no limite legado; o padrão `'Cancelar'` do `Dialog` continua em português.
  O `AnchoredPanel` (`views/bots/parts.tsx`) aceita o mesmo `language`.
- Erro que a interface escreve e guarda no estado de uma folha fica como chave do catálogo com os parâmetros e é
  traduzido na renderização (`ErroGuardado`/`useTextoDoErro` em `views/marketplace/pieces.tsx`):
  trocar o idioma com a folha aberta muda a mensagem também. O texto cru que vem do Hub fica como veio.
  Prova: `src/renderer/lib/ajustes-bidi.test.ts` (cartão de aprovação e confirmação da chave SSH em árabe).
- Estado que a interface escreve e guarda (loja, regra, `useState`) é uma `Frase` de `lib/fork-i18n/frase.ts`
  (`frase`, `plural`, `juntas`, `cru` para o texto do Hub, `ErroFrase` para erro lançado pela interface). Padrão único:
  a `Frase` é dado, `{ chave, params }` com a chave tipada (`ChaveTexto`; no plural, `{ plural, n, params }` com
  `ChavePlural`) e os parâmetros conferidos pelo tipo, sem closure nem texto pronto; a tela chama a função pura
  `textoDe(frase, idioma)` ao desenhar. Por ser dado, a frase pode ser clonada, serializada e comparada. Parâmetro texto
  (puro ou `cru`) é dado e entra isolado (FSI…PDI) em qualquer idioma; parâmetro `Frase` sai traduzido no mesmo idioma.
- Fontes, sem baixar nem redistribuir nada: a fonte escolhida em Aparência vem primeiro; depois, para o que ela
  não cobre, PingFang SC / Hiragino Sans GB (chinês) e Geeza Pro / Al Nile (árabe), que estão na
  [lista de fontes do macOS](https://support.apple.com/en-ie/122869) (podem estar instaladas ou só sob demanda;
  não há garantia de cada uma em todo Mac), Noto no Linux e `sans-serif` como reserva genérica do sistema.
  A ordem muda com `:lang(zh-CN)` e `:lang(ar-AE)`.

## Gate (CI)

`apps/desktop/tools/textos-gate.ts` roda dentro do `npm test` do desktop (`src/shared/fork-textos-gate.test.ts`),
no job `desktop` do CI e no hook. Falha quando:

1. um idioma não tem uma chave comum do pt-BR, tem chave a mais, texto vazio ou `{parâmetros}` diferentes; ou um
   plural não tem exatamente as categorias do idioma (sem exigir forma que o idioma não usa, como `one` em zh);
2. (catálogos por área, `conferirModulos`, antes de aceitar o agregado) uma chave está em duas partes, fora do
   prefixo do seu módulo ou sem módulo; uma pasta de idioma tem arquivo fora do registro, ou falta um módulo
   registrado; o índice não leva cada parte uma vez; um módulo tem algo além de `textos` com literais; o caminho
   antigo faz mais que reexportar; o catálogo em uso difere da união das partes; existe uma pasta em
   `fork-i18n/` fora da lista combinada; há um symlink dentro de `src`; há uma pasta `node_modules` dentro de
   `src`; ou há um arquivo `.js`, `.jsx`, `.mjs` ou `.cjs` dentro de `src` que a ponta da main não tem;
3. aparece **texto visível novo** fora dos catálogos em qualquer `.ts`, `.tsx`, `.mts` ou `.cts` do app: texto JSX, atributos
   visíveis e de acessibilidade (`aria-label`, `title`, `placeholder`, `alt`, `label`…), props de texto
   (`label: '…'`) e frases soltas (mensagens de erro, por exemplo);
4. o inventário do legado tem uma entrada que já saiu do código.

O legado está em `apps/desktop/tools/textos-legado.json`, por arquivo, texto e número de vezes (na criação,
igual ao código da `main` incorporada). Ele só encolhe: ao migrar uma tela, apague as entradas com
`node --experimental-strip-types tools/textos-gate.ts --encolher` (nunca acrescenta). Num PR, o inventário é
comparado com o texto solto do **merge-base** (`git merge-base` entre o HEAD e a ponta de `GITHUB_BASE_REF`),
não com a ponta da main na hora do teste: entrada ou contagem que o próprio PR acrescentou falha. O mesmo PR
simula a junção com a ponta da main (`git merge-tree --write-tree`, sem checkout; não altera refs, índice nem arquivos; pode criar objetos soltos).
Quem fica fora da checagem, no PR, é o valor que o código realmente exporta. Idiomas e módulos que só a main
já registrou são lidos desse commit, que já foi revisado. A junção simulada usa a soma das duas listas. Comentário
ou variável local no arquivo do PR não tira arquivo nenhum da checagem. Um módulo que a main já registrou não
vira texto solto num PR atrasado. Texto solto novo falha; sobra criada pelo PR (tirou o texto e não rodou `--encolher`) falha; sobra que já vinha
da main (o texto existia no merge-base e a main o removeu) só avisa; conflito em `textos-legado.json` não
reprova, porque esse conflito já bloqueia o merge. Base inalcançável num PR continua falhando. No hook, no
push da main e numa cópia sem `.git`, nada disso muda: vale a conferência estrita da árvore, e o aviso
de base indisponível segue igual. O CLI local usa o merge-base com `origin/main` e a mesma simulação.

As chamadas Git do gate e de sua fixture removem todas as variáveis de ambiente `GIT_*` herdadas,
preservando `PATH` e as demais variáveis. Assim, o repositório é escolhido pelo diretório do comando,
mesmo dentro do pre-commit. O JavaScript em `src` que a main já tinha fica em
`tools/javascript-legado.json`, lido da própria árvore: vale também onde não há Git (snapshot do hook, cópia por
`git archive`). JavaScript fora da lista falha; entrada da lista sem o arquivo também falha
("sobra"), e com Git a lista não pode crescer em relação ao merge-base (só encolhe). O arquivo é obrigatório e `arquivos` tem de ser um array de textos: ausente ou fora do formato, o gate falha com "javascript-legado.json inválido" (no commit da base, a ausência cai no JavaScript que o commit tinha). A regressão roda a fixture e a comparação com a base num subprocesso
contaminado por outro repositório descartável e confere que HEAD, refs, índices e arquivos dele
continuam idênticos. Base indisponível num PR continua reprovando; snapshot sem Git mantém o aviso.
Após a remoção do ElevenLabs, o `--encolher` retirou somente a ocorrência já ausente do código
(`tempo-real-ws.ts`: “Pedido de ferramenta inválido.”, de 2 para 1), sem acrescentar legado.

Exceção pontual só para **dado** em posição livre (nunca texto JSX, atributo visível/acessibilidade ou prop de
texto): comentário de verdade `textos-ok: <motivo>` junto do literal ou da declaração que o contém.

### Listar o que falta migrar

```sh
cd apps/desktop
node --experimental-strip-types tools/textos-gate.ts --listar                 # todas as áreas
node --experimental-strip-types tools/textos-gate.ts --listar --area sessoes  # uma área
node --experimental-strip-types tools/textos-gate.ts --listar --json          # para tradutores e PRs de troca
```

Saída: `arquivo:linha<TAB>texto`, agrupada por área (`idiomas`, `entrada`, `ajustes`, `sessoes`, `conversa`,
`criar-bot`, `equipes`, `rotinas`, `whatsapp`, `extensoes`, `spaces`, `main`, `outros`), pelo caminho do arquivo
(`tools/textos-gate.ts`, `AREAS`): Sessões = `views/fork/sessoes`, `views/fork/abas` e `views/fork/painel-direito`;
arquivos compartilhados ficam em `outros`. Não é análise de fluxo: diz onde o texto está escrito. Só lê.

O que estiver na `main` quando a base for incorporada entra no inventário inicial
(bootstrap pelo código da `main`); depois, cada frente troca seus consumidores para `useT` em PR curto e o
inventário encolhe. Depois da base na `main`, texto novo solto falha para todos, sem exceção geral.

Fora do gate, cada um com motivo: testes e fixtures, a demo do site (`src/renderer/demo/`, dados fictícios) e
os próprios catálogos: só os índices e módulos declarados no registro, cuja forma o item 2 confere. O resto de
`lib/fork-i18n` (motor, estado, registro e os caminhos antigos) continua lido. Na comparação com a base, o código
da base é lido com o scanner do candidato em modo `base`: lá os seis arquivos planos ainda eram o catálogo inteiro
e ficam de fora; a forma nova não é exigida da base. O inventário (`textos-legado.json`) não muda por mover
catálogos: eles nunca entraram nele.

**Limites conhecidos:** o verificador lê o código, não a tela. Identifica dado × produto pelo lugar (imports,
tipos, chaves, comparações, `className`, chamadas de log/IPC/armazenamento são dados) e por forma de frase.
Texto montado em tempo de execução, palavras soltas em constantes ou objetos fora de props de texto (ex.:
`const x = 'Livre'`), rótulos minúsculos sem acento e mensagens que o Hub manda prontas podem escapar (limite
conhecido; sem análise de fluxo). Não prova tradução completa: o inventário zerado e a conferência visual por
tela, sim.

