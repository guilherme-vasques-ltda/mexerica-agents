# Berga fixo

> **Nome:** o principal fixo, antes chamado Donna, agora se chama Berga ("o Berga"). No Hub, a renomeação automática só vale se o nome atual for "Donna", sem mudar conversas, memória e configurações.

## Quem é o Berga

O Berga é o bot principal configurado no Hub (`primaryBotId`). Num Hub novo ele é criado pelo
próprio Hub. Num Hub preservado, é a principal que o dono já tinha escolhido. Enquanto
não existe principal vivo, o dono ainda pode escolher um; a partir daí ele fica fixo.

## O que muda, quem muda

| Grupo | Campos | Quem muda |
| --- | --- | --- |
| Aparência | rosto (`avatarConfig`), cor e forma antigas, fixar, posição na lista, notificações, seções da barra lateral | livre |
| Do dono | motor, conta de IA, comando, modelo, opções do modelo, modo rápido, pasta, Pedir/Liberar, skills, conectores, logins, computador, memória compartilhada e do motor, **Suas preferências para o Berga** (`ownerPreferences`; texto da tela) | só o dono, pelo app |
| Fixo | nome, instruções base (`description`), função, Space, poder de gestão, ocultar/arquivar, origem, data, apagar, trocar ou desmarcar a principal | ninguém |

- **Erro estável:** a recusa é `donna_fixed` ("O Berga é fixo; só a aparência muda."), sem eco do pedido.
- **Equipes:** o Berga fica na raiz, como gerente e reportando a Você. Nenhuma gravação do quadro
  o coloca abaixo de outro bot (`primary_protected`).
- **Nome reservado:** com o Berga vivo, nenhum outro bot pode ser criado ou renomeado como Berga
  (`donna_reserved`); "Donna" fica livre para outros bots. A comparação ignora caixa, acentos,
  espaços e pontuação: "Bérga", " BERGA " e "b e r g a" são recusados. "Bergamota", "Iceberga" e
  "Berga Assistente" passam: o nome só é recusado quando, normalizado, é exatamente "berga".
  Homóglifos comuns do cirílico e do grego contam como as letras latinas. Um bot antigo que já
  tinha esse nome continua salvo. O Desfazer de uma gestão antiga passa pela mesma regra e não
  devolve o nome a outro bot.
- **Duplicar:** não existe ação de duplicar bot. Importar um modelo cria outro bot e passa pela
  reserva do nome.

## Instruções base e preferências do dono

As instruções base do Berga vêm sempre do Hub (`donna_fixa::BASE`), em todo Hub. O texto salvo no
perfil (`description`) não é mais lido para o Berga; ele fica guardado, sem ser apagado.

"Suas preferências para o Berga" é um texto do dono, com o mesmo limite das instruções (4.000
caracteres). Só entra no prompt da principal. No perfil ele vem **depois** da base, como dado: uma
string JSON com `<`, `>` e `&` escapados, avisando que complementa a base, que prevalece. Assim o
texto não abre nem fecha blocos como `<agent_profile_update>`. Em outro Space (`foreign`) as
preferências somem, como as demais instruções pessoais.

**Migração única.** Na primeira abertura do Hub com esta versão, ou quando o dono escolhe a
principal num Hub preservado, o Hub compara a descrição salva do Berga com a base:

- igual ou vazia: nada muda;
- diferente: o texto antigo é copiado para as preferências, sem apagar a descrição;
- se já houver preferências, elas ficam como estão e o texto antigo vem depois, com o separador
  um separador que marca as instruções antigas da Donna (anexar evita perder o que o dono escreveu);
- se a soma passar de 4.000 caracteres, nada é copiado.

O resultado fica num marcador durável (`donna.fixa.migracao.v1`: bot, situação e horário, sem o
texto) e no log do Hub. Com o marcador, a migração não roda de novo.

## "Só o dono, pelo app"

O Hub aceita os campos do dono apenas pelo caminho de configuração do app
(`Hub::update_bot_by_user_authorized`, usado por `updateBot` e pelo editor `donnaBotEditorSave`),
pela seleção de skills do app e pela remoção de skill do catálogo (`removeSkill`). Os três guardam a
autoridade ORIGINAL do pedido antes de qualquer fila e só marcam a escrita dentro de
`Autoridade::with_db`, que reconfere a autoridade na transação. Sem essa marca, o gatilho recusa.

Os recursos do dono que o Berga usa (cofre de credenciais e logins, registro de conectores, catálogo
de skills, WhatsApp, resumo do dia, limites de uso, compressão de contexto e as rotinas do Berga) seguem a mesma regra: `api::dispatch` guarda a autoridade original de um aparelho com
Control na entrada do pedido (`donna_fixa::pedido`), antes do cofre e de qualquer fila, e esses
pontos de gravação gravam dentro de `Autoridade::with_db`.

Classificação dos callers:

- **Aceitos como app do dono:** `Local` (o app no próprio computador do Hub) e `Device` com
  `Control` e a autoridade original do transporte. O app do dono pareado à distância, inclusive
  pela nuvem, é esse `Device`. A autoridade é capturada antes da fila (`team_lock`, verificação
  externa) e reconferida dentro da transação de escrita (Cloud → DB, como no SAAS-04), junto com a
  guarda de contas quando o pedido mexe na conta. Perder o acesso na fila recusa a escrita.
- **Recusados:** `Device` sem `Control` (inclusive só Tela), pareamento e bootstrap, que não chegam
  a esses métodos; a ponte de ferramentas de bot (`Ponte`), que não admite métodos de configuração;
  as ferramentas de bot (MCP `chat`/`manage`/`team`), que não editam a principal; rotinas e webhooks,
  que só executam turnos de bot com as mesmas ferramentas.

## Rotinas do Berga

O Berga pode criar suas próprias rotinas e recordações pelo MCP `routines`,
inclusive sem pedido humano, como os demais bots. Toda criação MCP conta na cota
padrão de 10 por bot em 24 horas móveis e gera aviso e cartão Manter/Cancelar no
Mission Board. Sem resposta a rotina continua ativa. Criação direta do dono no
app não conta. O limite por bot fica para uma etapa futura; nesta base só há o campo no Host.
Editar, pausar, excluir, disparar manualmente e girar chave conservam a exigência
de pedido humano e vínculo vivo da sessão de origem. Cada bot acessa somente suas
tarefas no seu Space. Filhas herdam a cadeia e o orçamento, sem nova raiz.

## Idioma da resposta

O Berga responde no idioma escolhido no app por cada pessoa, não mais
sempre em português. Ele continua fixo: na base só a frase do idioma mudou, para "Responda no idioma
do app da pessoa (pt-BR, en, fr, es, zh-CN ou ar-AE), indicado a cada turno; sem indicação, em
português do Brasil". Para quem usa em português, nada muda.

- **De onde vem:** o app manda `donnaAppLanguageSave {language}` ao conectar e a cada troca em Ajustes →
  Geral → Idioma. O Hub só aceita os seis códigos, escritos exatamente.
- **Onde fica:** uma chave por app no Hub: `donna.appLanguage.local`, o computador do Hub, que é o
  idioma do dono, e `donna.appLanguage.device.<aparelho>`, uma por aparelho. A chave do aparelho sai
  junto com ele na revogação, e uma chave de aparelho que não existe mais é ignorada na leitura.
- **Gravação segura:** o aparelho grava sob a autoridade ORIGINAL do transporte (a do canal
  autenticado), conferida na mesma transação da escrita (`Autoridade::proprio_with_db`), sem exigir
  Control, sem virar Local e sem buscar concessão nova. Grava junto a geração da concessão; na
  leitura, valor de geração diferente da atual é ignorado. Revogar no meio do pedido não grava, e
  uma concessão nova na mesma chave não herda o idioma do pedido antigo.
- **Quem muda:** cada app grava só o próprio idioma. Um aparelho sem Control também grava, porque é
  preferência dele e não dá poder sobre o Berga (classe `Proprio` em `donna_fixa::metodos`). A ponte
  de ferramentas de bot (inclusive em rotinas), o pareamento e a instalação não gravam.
- **Na mensagem:** todo ingresso humano carimba `appLanguage` com o idioma guardado de quem mandou,
  a partir de `idioma_app::do_chamador`, antes de gravar (`carimbar` nos envios; o cartão grava pelo
  `extra` server-owned). Um `appLanguage` no
  corpo do pedido é ignorado. Carimbam: `send` (conversa principal), `donnaConversationSend`
  (conversas adicionais) e `donnaConversationChoiceAnswer` (resposta ao cartão de escolha, que conta
  como mensagem humana). Caem no dono de propósito: canal/grupo, mensagens de fora (WhatsApp),
  rotinas, pedidos de outro bot e avisos internos.
- **No turno do Berga:** um aviso por turno ("Idioma da resposta neste turno: … (código)"). Vale o
  idioma da **última mensagem humana** do turno. Numa conversa com várias pessoas, é o de quem mandou
  por último; se esse idioma não for conhecido, vale o do dono. Turno de canal, mensagem
  de fora (WhatsApp), rotina ou pedido de outro bot seguem o idioma do dono; sem nenhum, pt-BR.
- **Só o Berga**, neste passo. Os outros bots seguem como antes.
- A frase de memória do manual ("siga o idioma pedido no perfil") continua valendo.
- A migração trata a base anterior (sempre em português) como base: não vira "Suas preferências".

## Modelo de cada conversa do Berga

Trocar o modelo de uma conversa do Berga (principal ou sessão extra, `sessionEngineSet`) é só do dono
pelo app: o próprio computador do Hub ou o app pareado com Control, a autoridade original do transporte e a
autorização de contas de IA (`engine.accounts`, SAAS-04), reconferidas na mesma transação da gravação. Aparelho
sem Control ou revogado, Ponte e ferramentas de bot, rotinas, pareamento e bootstrap são recusados sem mudar a
conversa. Os outros bots seguem a mesma regra que já tinham.

Encerrar ou continuar uma sessão extra do Berga (`sessionClose`/`sessionContinue`) é conversa, não
configuração do Berga (classe `Comum`): o computador do Hub ou o app com Control, com a autoridade original
reconferida na transação, sem `engine.accounts`. Nenhum gatilho do Berga fixo muda. A configuração do arquivamento
automático (`sessionAutoCloseSave`) é `Dono`; a leitura (`sessionAutoCloseConfig`), `Leitura`. Responder a um cartão de
escolha numa extra do Berga (`sessionChoiceAnswer`) também é `Comum`, com a mesma guarda original na transação e
sem `engine.accounts`. A pergunta nasce do `mostrar_componente` da própria Berga na extra (selo do
Hub para a extra exata), sem privilégio novo nem mudança do Berga fixo.

As ações por mensagem na fila (`queuedRemove`/`queuedEdit`/`queuedMove`/`queuedSendNow`) também são
conversa (classe `Comum`): a autoridade original é capturada na RPC e reconferida na transação do ator, antes do
efeito; aparelho revogado, mensagem já despachada ou revisão velha são recusados sem mudar nada.

## Escrita sem dono e fail-closed

- `Hub::update_bot`, usado por caminhos de bot (`chat::profile`, nomeador), não leva a marca do dono:
  no Berga, os gatilhos recusam o que é travado ou só do dono. A marca só existe com a autoridade
  real do pedido (`update_bot_by_user_authorized` com o `owner` capturado).
- `donna_fixa::pedido::gravar` é fail-closed: um aparelho sem Control ou sem a autoridade do
  transporte não grava nenhum recurso do dono. Local e as tarefas internas do Hub gravam como antes.
- Tabela única de métodos (`donna_fixa::metodos`): cada método que o Hub atende, nos roteadores
  centrais e nos módulos encaminhados (`handles`, `METHODS`, `if method == …`, `method.eq(…)`,
  `if let`, apelidos de `method`), tem uma classe, e um teste falha se aparecer método sem classe (há
  também um teste que prova a falha em cada forma). É inventário por leitura do código, não trava de
  autorização: roteamento por valor calculado, apelido passado a outra função ou macro nova escapam.
- Retomar uma espera de limite de uso: a autoridade do pedido vai até o ator; a retomada grava o
  checkpoint sob essa mesma autoridade, na mesma transação das checagens. Revogar entre a conferência
  e a gravação não retoma. Outro teste revoga o aparelho antes da gravação em todo método `Dono` e
  exige erro e estado inalterado.

## Limites conhecidos

- **Token local (`~/.donnabot/token`).** O Hub aceita como `Local` quem apresenta esse token. Qualquer
  processo do mesmo usuário do sistema (inclusive o motor de um bot, uma skill ou um MCP com shell)
  pode lê-lo e chamar a API local como se fosse o app do dono, mudando os campos só do dono. Fechar de
  verdade exige que o app prove que é ele (credencial própria do desktop).
- **Login legado dos motores além de Claude/Codex** segue aceitando o aparelho com Control do dono
  (exceção à SAAS-04).
- **Um só tipo de autoridade do dono** em toda a API (permissões lidas da tabela de métodos) e um
  registro único de métodos usado pelo dispatch e pela classificação são o refactor seguinte.

## Onde fica a trava

- `host/src/fork/donna_fixa/`: lista única (`TRAVADOS`, `DO_DONO`, `APARENCIA`), gatilhos SQLite
  recriados a cada abertura do Hub, marca do dono (`como_dono`), reserva de nome, preferências no
  prompt e tradução do erro.
- Os gatilhos olham só as colunas `id`, `config` e `deleted` de `bots`, o `DELETE` do Berga e a
  chave `team.settings.v1`. Não lidas, sessão, revisão, ordem e avatar continuam gravando.
- O editor do Hub (`donnaBotEditor`) não oferece nome, instruções, função nem apagar para o Berga.
  O app esconde ou desabilita esses controles e mostra o aviso.
- Textos do app pela base de Idiomas (`useT`), chaves `botSettings.donnaFixa.*` nos seis catálogos.
