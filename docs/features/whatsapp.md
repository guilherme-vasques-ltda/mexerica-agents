# WhatsApp por bot (não oficial) — ajustes no Hub

Existem os ajustes, o número de cada bot (conectar, manter conectado,
desconectar), o download do wacli oficial, as telas no app (ajustes, conectar e
desconectar o número), as **mensagens de texto recebidas**, que ficam numa **caixa do
WhatsApp própria de cada bot**, fora do chat normal, e o **envio de respostas com
fila gravada**: o bot responde a quem escreveu, dentro dos limites, em dois modos por bot
— "peço antes de enviar" (cartão; o dono aperta Enviar; rotina não envia) e "enviar
sozinho dentro dos limites" (só aqui a rotina envia, e só para conversas marcadas pelo
dono). Áudio, imagem e outros tipos chegam como "recebida (em breve)". O dono também
escreve direto no mini WhatsApp.

## Direção do envio autônomo

As regras abaixo registram a direção decidida e as lacunas; não descrevem capacidades já
entregues, salvo onde indicado:

- **Mesma conversa**: a resposta autônoma só pode ir para a conversa da mensagem recebida.
  O envio já exige uma mensagem guardada da mesma conversa e conta, e reconfere isso na fila.
- **Grupo marcado nasce desligado**: marcar um grupo não liga por si a resposta autônoma. A
  resposta autônoma por grupo é uma opção própria (abaixo, `donnaWhatsappGroupAuto`), que nasce
  desligada, inclusive nas marcas anteriores, e o dono liga ou desliga por grupo no menu dele. A
  marca de conversa individual mantém o comportamento atual.
- **Permissão mínima do turno**: o turno autônomo que lê conteúdo de terceiros e responde
  deve ter somente as ferramentas necessárias ao WhatsApp, sem conector, computador ou
  cofre. O corte por execução ainda não existe; sua forma depende de decisão própria.
  Limitar o transporte ou tratar a mensagem como conteúdo de fora não limita, por si só,
  as outras ferramentas do turno.
- **Autoridade de quem escreveu**: cada mensagem de entrada deve mostrar o chip Você,
  Conhecido ou Desconhecido. A identificação de Você depende de como o dono declara seu
  número (decisão pendente); não presumir que o número do bot seja o do dono.
  O chip ainda não está implementado e não transforma conteúdo de fora em ordem.
- **NO_REPLY**: registrar o sinal de que o bot não deve responder como ausência de envio,
  nunca como texto a mandar no WhatsApp. O contrato e a implementação desse sinal para o
  turno autônomo ainda estão pendentes.
- **Menção**: uma menção pode aparecer como `@número` no corpo; na versão observada, o wacli
  não tem coluna de menção. Reconhecimento, normalização e efeito ainda estão pendentes; esta
  spec não manda implementá-los agora.
- **Contexto pendente**: o tratamento de mensagens e contexto que aguardam uma resposta
  autônoma ainda precisa de contrato próprio. Não presumir que mensagem recebida inicie um
  turno: hoje o recebimento não o dispara, e o envio sozinho ocorre em turno existente do
  dono ou de rotina viva.

A deduplicação e a conferência da entrega continuam obrigatórias: envio incerto não
é reenviado automaticamente.

## O que o Hub guarda

- **Plugin ligado ou desligado**, desligado por padrão. É a instalação `whatsapp`
  do registro de plugins (`host/src/fork/plugins`), uma por Hub.
- **Modo**: "Só o Berga" (código `donna_only`) (padrão) ou "Um número por bot".
- **Por bot** (tabela própria `donna_whatsapp_bots`):

| Opção | Padrão | Regra |
| --- | --- | --- |
| Ver conversas | desligado | — |
| Responder conversas | desligado | exige "Ver conversas" |
| Ver grupos | desligado | — |
| Responder grupos | desligado | exige "Ver grupos"; na fase 1, só quando respondem ao bot |
| Como responde | aguarda aprovação | "peço antes de enviar" (padrão; cartão, o dono aperta Enviar) ou "enviar sozinho dentro dos limites" (a rotina envia; só aqui `Origin::Routine` é aceito) |
| Intervalo entre mensagens (X) | 15 s | 8 a 300 s (piso de 8 s); variação de +0 a 30 % **acima** de X |
| Limite de mensagens por hora | 30 | 1 a 200 (trava de 200; o dono pode baixar) |
| Limite de mensagens por dia | 100 | 1 a 500, por bot × número, ao lado do por hora |
| Horário de silêncio | ligado, 22h–8h | no fuso do dono; fora do horário o item fica "segurado" e sai na abertura; o envio direto do dono não é segurado |
| Guardar as mensagens por | 30 dias | 7, 30 ou 90 dias |
| Transcrever áudios | ligado | — |

  Não começar conversa e não mandar para vários contatos são travas fixas, não opções.
- **Regras da conexão:** limite e intervalo são da conexão (bot × número), não
  da rotina. O Hub recusa fora das faixas venha de onde vier (tela, rotina, bot, app antigo), com
  `invalid_field` e a regra em português; a tabela também tem as faixas no próprio banco. Na
  atualização, ajustes antigos com espera de 3 a 7 s sobem para 8 s; 8 s ou mais e o limite
  antigo (1 a 60) ficam iguais; nada desce. A fila por conexão (`pace.rs`, tabela
  `donna_whatsapp_pace`) guarda as últimas reservas e diz a cada mensagem "sai agora" ou
  "espera até tal hora", somando todas as rotinas do número. Ela também
  impõe o **limite por dia** (abaixo) na mesma reserva.
- **Responder:** a opção "Responder" do dono liga de verdade a concessão
  `whatsapp_reply_chats`/`whatsapp_reply_groups` do registro de plugins; desligar revoga.
  Pausar o bot ou desligar o plugin também revoga, e a fila pendente cai junto (abaixo).
- **Envio de respostas:** o envio está **aberto**
  como produto. O bot só responde a quem escreveu para o número; nunca começa conversa,
  nunca manda para lista, só texto de até 4096 caracteres, sempre citando a mensagem
  recebida (`--reply-to`; o wacli exige). Conteúdo recebido continua **conteúdo de fora**:
  nunca vira ordem. Dois modos por bot, escolhidos em "Como responde":
  - **"Peço antes de enviar"** (padrão, `approval`): o bot prepara o rascunho de 90 s
    (`preparar_resposta_whatsapp`, a mesma captura `WhatsappCardSource` de antes) e o
    envio só acontece quando o dono aperta **Enviar** no cartão
    (`donnaWhatsappSendDraft`, só este computador). O gesto do dono no cartão é a
    permissão: entra na fila como origem `owner_card`, que não pede marcação nem modo
    autônomo — só as conferências gerais. Rotina **não** envia neste modo: a ferramenta
    `whatsapp_reply` recusa com `whatsapp_reply_mode`. "Agora não" fecha o cartão sem
    enviar nada.
  - **"Enviar sozinho dentro dos limites"** (`autonomous`): aqui — e só aqui — a rotina
    pode enviar (`Origin::Routine` é aceito). O bot chama a ferramenta `whatsapp_reply`
    `{whatsappChatId, messageId, text}` num turno do dono ou de rotina viva do próprio
    bot (`whatsapp_owner_turn_required` para `ask_bot`, grupo alheio e cia). A chamada
    já confere modo, conta conectada, mensagem guardada daquela conta, concessão
    "Responder" e **conversa marcada**; depois enfileira com origem `bot_human` ou
    `bot_routine`. **Triagem:** no modo sozinho o bot só responde a conversas que o dono
    marcou; as demais continuam só na caixa dele.
  - **Fila de saída gravada** (tabela `donna_whatsapp_outbox`, aditiva): id
    determinístico (bot + conta + conversa + mensagem respondida + digest do texto)
    para dedup — a mesma mensagem pedida duas vezes é um envio só. Estados: `pending`,
    `held` (segurada: silêncio, limite, ritmo, sem conexão), `sending` (gravado na
    mesma transação que reserva o ritmo, **antes** do `wacli`: o dreno só pega
    `pending`/`held`, então nenhuma falha posterior devolve o item à fila), `sent`,
    `uncertain` (o envio não confirmou ou a gravação do resultado falhou — **nunca
    reenvia sozinho**, vira item visível para o dono decidir "Enviou"/"Não saiu"),
    `expired` (validade de 24 h) e `cancelled`.
    O drain roda no Hub, restaura no boot e, **na vez de cada item, reconfere tudo na
    mesma transação que reserva o ritmo**: plugin ligado, conta conectada e a mesma
    encarnação, mensagem guardada que a conversa mandou, origem permitida pelo modo
    (cartão do dono ou autônomo + marcada), concessão "Responder", horário de silêncio
    e os limites (hora e dia). `Sent::NotStarted` volta a `pending` com espera crescente
    e teto; `Sent::Uncertain` para em `uncertain`. **Efeito externo nunca reenvia:**
    a confirmação (`Sent::Yes`) que não grava cai para `uncertain`; se nem isso grava,
    o item segue `sending` até o lease vencer e a varredura o marca `uncertain`;
    `sending` vivo num reinício também vira `uncertain` — em todos os casos o dono
    decide pela fila e o transporte é chamado uma única vez.
    O envio é um único `wacli send text --json --to=<JID>
    --reply-to=<id> --message=… --no-preview` (wacli v0.25.1), só se a conexão ainda é
    a mesma da reserva. O `sync` do número sobe com `--send-spacing 8s`, o piso do
    próprio wacli, abaixo da fila do Hub.
  - **Limite por dia**: além do por hora, `daily_limit` por bot × número (padrão 100,
    1–500), numa janela móvel de 24 h sobre as reservas do `pace`. Estourado, o item
    fica `held` até a janela liberar.
  - **Horário de silêncio**: `quiet_enabled`/`quiet_start`/`quiet_end` por bot (padrão
    ligado, 22h–8h), no fuso do dono (`quiet_time_zone` quando definido). Fora do
    horário o item fica `held` (`reason=quiet`) e sai na abertura. O **envio direto do
    dono** (abaixo) não passa pela fila e não é segurado.
  - **Conversas marcadas**: tabela `donna_whatsapp_marked`, por bot × conta × conversa.
    O dono marca/desmarca pelo menu ⋯ da conversa (`donnaWhatsappChatMark`, porta do
    dono); a lista de conversas já vem com `marked`. Desmarcar cancela os itens
    pendentes/segurados daquela conversa. Marcar só faz sentido no modo sozinho, mas
    fica gravado para quando o dono ligar. Grupo marcado nasce com resposta autônoma
    desligada, inclusive nas marcas anteriores à migração; o dono pode ligar ou desligar
    separadamente no menu do grupo (`donnaWhatsappGroupAuto`, só neste computador).
    Desligar cancela respostas autônomas pendentes ou seguradas desse grupo; o Hub exige
    a opção ligada ao enfileirar e reconfere na vez de enviar. Conversas individuais
    mantêm o comportamento anterior. O cartão aprovado pelo dono não depende dessa opção.
  - **O que esvazia/cancela a fila**: desligar o plugin cancela tudo (`plugin_disabled`);
    revogar "Responder", trocar de número ou sair do modo autônomo cancela o que for
    `bot_*` (`reply_revoked`/`not_allowed` na reconferência); apagar o bot apaga a fila
    dele.
  - **Leitura e decisão do dono**: `donnaWhatsappQueue {botId}` lista pendentes,
    seguradas, incertas e as enviadas da última hora (até 100); `donnaWhatsappQueueDecide
    {botId, itemId, decision}` — `sent` só para incerto (o dono confirma que saiu),
    `cancelled` para incerto/pendente/segurada. Decisão grava pela autoridade do pedido
    do dono e devolve a fila nova.
  - **Escrita direta do dono**: `donnaWhatsappSend {botId, whatsappChatId, text}`,
    só este computador (`LocalOnly`), fora da fila, do silêncio e dos limites — é o número
    dele, como no WhatsApp Web. **Quem executa é o Hub onde o número está** (o
    `wacli` roda lá, não no aparelho do app): a tela não promete "do seu aparelho".
    Exige número conectado e conversa que já escreveu para ele; responde citando a
    última mensagem guardada. `Sent::Uncertain` volta como `{"state":"uncertain"}`
    para o app avisar — o dono confere no WhatsApp antes de repetir; `NotStarted` é
    erro, nunca sucesso falso. Não há dedup de fila porque cada envio é um gesto
    explícito do dono; o caminho inteiro (autoridade local, estados de falha,
    incerto) segue as mesmas conferências do restante do envio.
    `Origin::Direct`/`OwnerDirect` segue **recusado** na fila do bot: o dono nunca entra
    como origem de resposta do bot.
- **Transcrição no computador**: provedor (OpenAI ou Gemini) e modelo para os áudios
  do WhatsApp. Sem escolha, vale o primeiro provedor com chave no Hub e o modelo de
  transcrição padrão dele. Não muda a escolha "deste aparelho" da tela Voz.

O fechamento individual do rascunho é atômico: remove o privado e fecha recibo e
projeção juntos. Se o banco estiver ocupado, tenta até três vezes com 10 ms entre
elas, sem travas na espera e sem renovar o prazo; nenhuma tentativa recusada
persiste fechamento. Erro SQL ou no commit reverte tudo e não é repetido.
Cancelamento durante a espera não deixa fechamento parcial. O app mostra
indisponibilidade no erro, sem conteúdo privado ou erro bruto, mesmo após o prazo.
Expirar no relógio do app não confirma limpeza física; só recibo fechado do Hub
mostra o rascunho como encerrado.
No StoreBusy final, a custódia fica no mapa, sem apresentação nessa chamada;
o timer para após a sequência única de tentativas, sem retry a cada segundo.
A limpeza física pode ficar pendente para a próxima leitura ou o startup, como
descrito no [manual do harness](manual-do-harness.md).
Na segunda conferência depois de abrir o cofre, banco ocupado só deixa a leitura
indisponível: descarta o texto, sem fechar ou limpar o rascunho. Mantém recibo,
privado, fonte e prazo original; uma próxima leitura válida pode mostrá-lo.
Expiração ou revogação continuam encerrando o rascunho, sem renovar o prazo.
O gesto do dono no cartão enfileira o envio (`donnaWhatsappSendDraft`,
só este computador); "Agora não" só fecha o rascunho.

## Regras

- No modo "Só o Berga", outros bots não podem ser ligados. Trocar para esse modo
  **pausa** os outros bots: os ajustes ficam guardados, mas o bot não pode nada.
  Voltar para "Um número por bot" tira a pausa.
- **Desligar o plugin** desliga ver/responder de todos os bots e sobe a geração da
  instalação. **Religar não restaura** nenhum acesso (contrato de plugins §7).
  Espera, limite e transcrição ficam guardados. Com o plugin desligado, nada do bot
  pode ser gravado. Quando existirem cartões de resposta, eles caem com a troca
  de geração.
- Apagar o bot apaga os ajustes dele.

## O número de cada bot

Um número ↔ um bot ↔ uma pasta. O Hub roda o wacli como processo separado, sempre
com `--store` apontando para a pasta do bot.

- **wacli**: o Hub procura o binário em `whatsapp/bin/wacli` dentro da pasta de dados
  (o Hub o baixa para lá, veja "Como o wacli chega") ou no caminho da variável `DONNABOT_WACLI`. Sem binário, o
  número aparece como **"wacli não instalado"** e conectar é recusado; nada mais quebra.
- **Pasta**: `whatsapp/<bot>/` dentro da pasta de dados do Hub, só do dono do processo
  (`0700` no Mac e no Linux; o wacli também abre os processos com máscara `077`). No
  Windows a pasta fica no perfil do usuário, que por padrão só o próprio usuário, o
  sistema e administradores leem; o Hub não muda essa permissão. A sessão
  (`session.db`) dá acesso total ao número e não é cifrada pelo wacli.
- **Conectar** (só o dono, com o plugin ligado e o bot permitido pelo modo): por
  **código de 8 letras** (o dono informa o número; o WhatsApp do celular pede o código)
  ou por **QR** (o app desenha o texto do QR). O Hub devolve o código ou o QR e depois o
  resultado: conectado, venceu, recusado ou cancelado. O QR se renova sozinho enquanto
  espera. Código e QR nunca vão para log nem para os avisos gerais do Hub; só a leitura
  do dono os mostra. O pareamento traz no máximo 1 dia de histórico.
- **Manter conectado**: com o número conectado, o plugin ligado e o bot não pausado, o
  Hub mantém um `wacli sync --follow` por bot. Se ele cair, volta com espera crescente
  (1 s, 2 s, 4 s… até 5 min). Desligar o plugin, pausar o bot (modo "Só o Berga") ou
  apagar o bot para o processo. Quando o Hub para, para todos. Se o Hub cair sem
  parar o wacli, na próxima vez ele encerra o processo que sobrou (Mac e Linux).
- **Estados do número**: nenhum, conectando, conectado, desconectado (caiu; o wacli
  tenta voltar), parado (plugin desligado ou bot pausado), precisa conectar de novo
  (o aparelho foi tirado no celular), pareando e wacli não instalado.
- **Desconectar**: avisa o WhatsApp (`auth logout`) e apaga a sessão local. As
  mensagens guardadas ficam. **Desconectar e apagar**: o mesmo e apaga a pasta inteira
  (sessão, mensagens e mídia). Se o aviso ao WhatsApp falhar, o Hub diz para o dono
  tirar o aparelho em "Aparelhos conectados" no celular.
- **Apagar o bot** faz o mesmo que "Desconectar e apagar". Pastas de bots que não
  existem mais também são apagadas quando o Hub inicia.
- **O número conectado é CONTA do registro de plugins** (`donna_plugin_accounts`,
  com um recurso `whatsapp_number` de mesmo ID). Cada pareamento gera uma conta nova;
  conectar de novo nunca devolve acesso antigo. A conta, o recurso e as concessões somem
  ao desconectar, ao desconectar e apagar e ao apagar o bot.
- **Número do telefone**: depois de conectar, o Hub pergunta ao wacli
  (`--read-only auth status --json`) qual número está pareado e guarda `+DDI…` junto da
  conta do pareamento (tabela do módulo, coluna `phone`). Só vale o número confirmado pela
  sessão: nunca o digitado no pareamento por código nem o nome do bot. Pareamento novo
  começa sem número até o wacli confirmar; sessão caída ("precisa conectar de novo") ou
  desconectada apaga o valor. A leitura `donnaWhatsapp` mostra `number.phone` (para a tela
  dizer "Conectado · +55 …"). O número nunca vai para log.

## Mensagens recebidas

- **Quem pode ver** é concessão do registro de plugins (`donna_plugin_grants`), por bot e
  número: `whatsapp_read_chats` ("Ver conversas") e `whatsapp_read_groups` ("Ver grupos").
  As opções do bot guardam a escolha do dono; o Hub concede quando o plugin está ligado, o
  bot não está pausado e a opção está ligada, e revoga quando qualquer uma muda. A
  checagem "este bot pode ver" é sempre a do registro; o módulo não tem controle paralelo.
  Só bots do espaço Pessoal recebem a concessão. A leitura mostra o resultado em `botCan`
  (sem número conectado, "ver" fica falso).
- **Como chegam**: o `wacli sync` avisa o Hub a cada mensagem gravada (webhook). O aviso é
  só "chegou algo": o Hub escuta só em `127.0.0.1`, numa porta escolhida na hora, e confere
  a assinatura HMAC com um segredo novo a cada início do `sync` do bot. Aviso com
  assinatura errada ou vindo de fora do computador é ignorado, e o conteúdo do aviso nunca
  é usado. O wacli só aceita o segredo por linha de comando, então ele aparece na lista de
  processos do próprio usuário: risco aceito no computador do usuário (ele muda a cada
  início). A mensagem é lida do `wacli.db` do bot **só para leitura** (o Hub nunca escreve
  nele), a partir de um cursor guardado por bot (`donna_whatsapp_cursors`). Se um aviso se
  perder, uma leitura de segurança a cada minuto pega o que faltou. Reler a mesma mensagem
  (por exemplo, depois de uma queda) não a duplica na caixa.
- **O que entra**: conversa individual só com "Ver conversas"; grupo só com "Ver grupos" e,
  nesta fase, **só a mensagem de grupo que responde a uma mensagem do bot** (para não
  inundar a caixa). As outras mensagens de grupo só passam pelo cursor e não são
  mostradas. Mensagens enviadas pelo próprio número, reações, apagadas, status e canais
  não entram.
- **Mensagens antigas**: o histórico trazido pelo pareamento nunca entra. O que chegou
  enquanto o computador ou o wacli estava fora (mais de 3 minutos antes da leitura) entra
  marcado "chegou enquanto desligado" e não será motivo de resposta automática.
- **Mensagens de voz transcritas**: com "Ver" e "Transcrever áudios recebidos" ligados,
  cada mensagem de voz que chega (inclusive as que chegaram com o computador desligado) entra
  na caixa como "transcrevendo". Uma por vez por bot, numa fila à parte que não segura a
  entrada de texto, o Hub baixa o áudio **só pelo wacli oficial**
  (`--read-only --json media download --chat --id --output`) para uma pasta temporária dentro
  da pasta `0700` do bot (`whatsapp/<bot>/transcrever/`), manda para o provedor e modelo de
  transcrição deste computador (Ajustes → WhatsApp; `audio/ogg`) e **apaga o arquivo logo
  depois**, com sucesso ou falha. A caixa guarda só o **texto** (até 4000 caracteres) e quem
  transcreveu. O mini WhatsApp mostra "Mensagem de voz", o texto e "Transcrito por
  OpenAI/Gemini". Estados: "Transcrevendo…"; "Longa demais para transcrever" (OpenAI acima de
  25 MiB, Gemini acima de 15 MiB de arquivo, que dão 20 MiB em base64; conferido antes de
  baixar, pelo tamanho que o wacli informa); "Sem transcrição: falta a chave em Ajustes → Voz";
  "Formato que não dá para transcrever" (só OGG); "Não deu para transcrever" (falha do
  provedor ou do download: **uma nova tentativa no máximo**, sem laço). Com a opção
  desligada, nada é baixado e fica só "Mensagem de voz". O Hub confere "Ver", "Transcrever", o
  plugin ligado e a conta (número) em que a voz chegou antes de baixar, de novo depois do
  download e antes de mandar o áudio ao provedor, e mais uma vez antes de gravar o texto. Se
  algo mudou (opção desligada, "Ver" revogado, plugin desligado ou outro número), o áudio é
  apagado sem ir ao provedor (sem custo), ou o texto que voltou é descartado, e fica só
  "Mensagem de voz". A transcrição é **conteúdo de fora**
  e vale os mesmos 30 dias. **O bot só lê a transcrição pela ferramenta de leitura do
  WhatsApp, quando pedido**, com as mesmas regras da mensagem. Fora desse caminho, nunca
  vai ao motor, à memória, à busca, ao histórico ou ao log (nem texto nem áudio). Foto e vídeo continuam "em breve".
- **Custo**: ligar "Transcrever áudios recebidos" **gera custo no provedor** (OpenAI ou
  Gemini), cobrado na conta do dono, a cada mensagem de voz recebida. Nenhum modelo novo: o
  mesmo de transcrição do computador.
- **Tipos e rótulos (conferidos no wacli v0.25.1)**: texto; mídia só como rótulo,
  sem conteúdo: "Foto", "Vídeo", "GIF", "Mensagem de voz", "Documento",
  "Figurinha", "Localização" e "Localização em tempo real", os tipos que o wacli marca
  (a voz ganha o texto transcrito, acima).
  O wacli não separa mensagem de voz de arquivo de áudio e não guarda a duração do áudio,
  então não há "(0:14)". **Chamadas recebidas** em conversa individual entram como um
  item da conversa ("Chamada de voz", "Chamada de vídeo"), uma por chamada, com a duração
  quando o wacli a informa; chamadas feitas pelo número e de grupo não entram. O wacli
  atualiza o registro da chamada na mesma linha (duração, direção, tipo); a cada leitura o
  Hub relê as chamadas dos últimos 7 dias e completa as que já estão na caixa. Uma chamada
  vista antes de o wacli saber a direção entra quando ele a completar; uma recusada por
  "Ver conversas" desligado não volta depois. Em grupo,
  cada mensagem diz se o remetente é **contato salvo** na agenda do celular; sem isso, o
  app mostra "~Nome" (nome do perfil), como o WhatsApp. **Sem foto de perfil**: o wacli
  não a traz na lista, a tela usa iniciais e o Hub nunca a baixa por outro meio. A conversa
  fixada é relida do wacli a cada leitura (no máximo a cada minuto).
- **Onde ficam**: na caixa do WhatsApp do bot (tabela própria `donna_whatsapp_messages`),
  **nunca no chat normal do bot**, nas Sessões, na memória ou no histórico consultável
  (busca/exportação), e não vão sozinhas ao motor: o único caminho até o bot é a leitura
  pedida pelo dono (abaixo). Guarda só o necessário: conversa ou grupo, quem mandou
  (nome e número), hora, texto, ID da mensagem, se chegou enquanto desligado e se o dono
  já leu. Tudo é **conteúdo de fora**, nunca ordem. Chegar mensagem não dispara turno
  do motor nem resposta.
- **O bot lê quando pedido**: duas ferramentas no servidor de conectores do
  motor, `whatsapp_list_chats` (conversas, da fixada e mais recente para a mais antiga) e
  `whatsapp_read_chat {whatsappChatId, limit?, before?}` (mensagens, da mais recente para a
  mais antiga, até 50 por chamada, paginadas). Só o número **daquele bot**, só bot agente do
  espaço Pessoal (outro Space é recusado pela guarda do dispatch e de novo pelo módulo). O
  acesso é a concessão "ver" do registro de plugins (`can_read`): conversa individual com "Ver
  conversas", grupo com "Ver grupos", e só as mensagens da **conta atual** do número: depois
  de trocar de número, o que ficou guardado do número anterior continua na tela do dono, mas
  o bot não lê. E só **num turno pedido pelo dono ou numa rotina do próprio bot**: o Hub
  confere a origem que gravou na admissão do turno, com o mesmo token da chamada; pedido de
  outro bot (`ask_bot`), grupo, recuperação ou mensagem que não é do dono é recusado
  (`whatsapp_owner_turn_required`). Mensagens vencidas nunca saem. É só
  leitura: não marca como lida, não grava nada, não envia e não cria cartão. Na conversa
  adicional (outro contexto) as ferramentas não existem. O resultado vai **marcado como
  conteúdo de fora** (`_meta."donnabot/guard".origin = "external"`, como os retornos de
  conectores) e a chamada marca a exposição externa do bot; instrução dentro de mensagem
  nunca vira ordem. O motor recebe o resultado inteiro no turno, mas o
  registro da ferramenta do turno (os passos do bot, que não vencem) guarda **só um resumo**
  ("Leu 2 mensagem(ns) de 1 conversa do WhatsApp"), sem texto, nome ou número.
  Nada disso entra no histórico consultável. Limites: o que o bot escreve na resposta a partir
  das mensagens vai para o histórico e pode ir para a memória automática, como qualquer
  resposta ("não copie para a memória" é orientação, não barreira); e o motor (Claude Code ou
  Codex) guarda o transcript da sessão nos arquivos dele, fora do prazo do Hub, como nos
  conectores. Fora disso, nada mudou:
  busca, memória e exportação continuam sem a caixa.
- **Quem vê pelo app**: só o dono (este computador ou aparelho com Control), e só para
  bots agentes do espaço Pessoal. Fora a leitura pedida pelo próprio bot, nenhum bot, motor,
  ferramenta ou outro membro chega à caixa. Funções:
  - `donnaWhatsappChats {botId}`: conversas como no WhatsApp: **fixadas primeiro** (`pinned`,
    o alfinete do celular), depois a mais recente. Cada uma traz o nome do contato ou grupo,
    o número (só conversa individual, para quando não há nome), a última mensagem (quem
    mandou, se é contato salvo, texto ou rótulo da mídia, chamada) e as não lidas, mais o
    total de não lidas do bot;
  - `donnaWhatsappMessages {botId, whatsappChatId, before?, limit?}`: mensagens de uma conversa, da
    mais recente para a mais antiga, em páginas de até 100 (`next` é o `before` da próxima);
  - `donnaWhatsappMarkRead {botId, whatsappChatId}`: marca a conversa como lida e devolve as não
    lidas que sobraram;
  - `donnaWhatsappChatErase {botId, whatsappChatId}`: "Apagar esta conversa deste
    computador". Apaga só aquela conversa, só daquele bot, da caixa do Hub, e depois a cópia
    do wacli pelo comando oficial (abaixo). Devolve `erased` (contagem da exclusão inicial
    do Hub), `unread` e `wacli`: `done`, `skipped` (o wacli não tinha a conversa) ou `failed`.
    `failed` inclui timeout ao aguardar outro apagar ou parar o leitor, erro no reapagar e
    falha no cleanup. Seu aviso não confirma a exclusão completa e pede tentar novamente:
    mensagens podem continuar no Hub ou na cópia do wacli. No app, `failed` mantém a
    conversa na lista, inclusive após avisos do Hub durante a tentativa, e o aviso pede
    para tentar apagar de novo; a tela não diz que a
    conversa saiu. Nada muda no celular nem no
    WhatsApp da outra pessoa. Antes de `done`, o leitor já parou e o Hub reapagou qualquer
    lote capturado antes do pedido, antes de limpar a cópia do wacli. Timeout ou
    erro no reapagar não executa o cleanup. Mensagem nova depois do retorno entra normalmente.
  O JID da conversa vai em `whatsappChatId`, nunca em `chatId` (que é conversa do Hub).
  Mensagem nova, marcar como lida e prazo vencido geram o aviso `whatsappChanged` (só o ID
  do bot), entregue só ao dono e só no stream do Space do bot. A leitura `donnaWhatsapp` traz, por bot, o estado do número e
  `inbox.unread` (não lidas), para a barra mostrar a contagem.
- **O que acontece com as guardadas**: desligar "Ver conversas/grupos" para de receber
  novas e mantém as antigas até o prazo; "Desconectar" mantém; "Apagar esta conversa
  deste computador" apaga só aquela conversa; "Desconectar e apagar" e apagar o bot apagam
  tudo daquele bot.
- **Prazo de guarda**: por bot, **7, 30 (padrão) ou 90 dias**, contados de
  quando a mensagem chegou ao Hub (`donna_whatsapp_bots.retention_days`; valores em
  `store::RETENTION_DAYS`). Na tela: Configurações do bot → WhatsApp → "Guardar as mensagens
  por". Trocar o prazo vale para o que já está guardado: **encurtar** tira das leituras na
  hora e apaga na próxima limpeza; **alongar** estende só o que ainda vale e nunca traz de
  volta o que já venceu (a troca apaga as vencidas na mesma transação, sem esperar a
  limpeza). A troca avisa o app (`whatsappChanged`); a página de mensagens traz `receivedAt`
  de cada uma, `keepAfter` (agora − prazo) e `retentionRev` (versão do prazo, sobe a cada
  troca). O app tira da tela as mensagens já carregadas que ficaram para trás do corte e,
  quando a versão muda, troca tudo o que tinha carregado pela página nova, em vez de juntar.
  Assim, encurtar e depois alongar (30 → 7 → 90) não deixa na tela o que o Hub já apagou,
  mesmo com uma releitura atrasada. O que passou do prazo nunca é entregue nem contado, e sai da caixa
  numa limpeza no início do Hub e de hora em hora, com ou sem número conectado, plugin ligado
  ou bot pausado. Mídia: o Hub não baixa mídia nem guarda arquivo temporário (o `sync` roda
  sem `--download-media`), então não há arquivo a apagar.
- **A cópia do wacli (`wacli.db`)**: o wacli guarda a cópia dele das mensagens na pasta do
  número (o histórico do pareamento é limitado a 1 dia e 20 por conversa). **O Hub nunca
  escreve no `wacli.db`.** No wacli v0.25.1 há só dois comandos oficiais que apagam lá:
  `chats cleanup --jid <JID>` (uma conversa inteira, com mensagens e chamadas, em cascata;
  `cmd/wacli/chats_cleanup.go`, `internal/store/chats.go:180`) e `chats cleanup --days N`
  (conversas inteiras sem nenhuma atividade há N dias). Não há comando que apague
  **mensagens** antigas por idade (`messages purge` só apaga mensagem já marcada como
  apagada; `messages delete` apaga no WhatsApp, o que não queremos). Por isso:
  - "Apagar esta conversa" usa `chats cleanup --jid <JID> --confirm`. O comando precisa da
    trava do store, então o Hub para o `sync` do bot por alguns segundos, roda o comando e
    liga o `sync` de novo. Com um pareamento em curso, não interrompe e responde `failed`.
    Dois apagares do mesmo bot são serializados: o segundo espera até 10 s pelo primeiro
    e depois também drena e reapaga sua própria conversa antes do cleanup. A parada do
    leitor tem seu limite de 10 s; vencer qualquer espera retorna `failed`, sem confirmar
    exclusão completa. Nenhuma trava de estado ou banco atravessa essas esperas.
  - O prazo de guarda vale só para a caixa do Hub. **A cópia do wacli fica até "Desconectar
    e apagar"** (ou "Apagar esta conversa"). Pendência: decidir se o Hub roda
    `chats cleanup --days <prazo>` uma vez por dia (apaga só conversas paradas há mais que o
    prazo, com o `sync` parado alguns segundos) ou se pede ao projeto wacli um comando que
    apague mensagens por idade.

## Número do bot escondido na tela

O número conectado do bot aparece **mascarado** em todo lugar do app em que é mostrado: no topo da aba
WhatsApp do painel, no chip da tela grande e no aviso de lista vazia. A forma é "+55 •• ••••-••50",
só com os 2 últimos dígitos, e vem de um componente único (`NumeroBot`). É uma máscara, não um desfoque:
com o número escondido, o número real não está no DOM, no texto copiável nem no `aria-label`. O leitor
de tela lê "Número escondido, termina em 50". O olhinho ao lado ("Mostrar número"/"Esconder número")
revela o número, que volta a esconder sozinho depois de 10 s, quando a janela perde o foco, quando a
página fica oculta ou ao trocar de tela ou de aba. Nada é salvo como "sempre visível". Ajustes →
Plugins, Configurações do bot e o pareamento não mostram o número do bot. O campo em que o dono digita
o número para receber o código de pareamento continua visível. O motivo é evitar que um print mostre o
número inteiro.

O manual do harness diz ao bot que o prazo e o "Apagar esta conversa" são só do dono, que
mensagem antiga que não aparece pode ter passado do prazo ou sido apagada deste computador, e que,
ao responder, ele chama quem não tem nome salvo pelo final do número e só escreve o número
inteiro se o dono pedir.

### Contatos, grupos e conta de saída

O mesmo componente (`NumeroBot`) esconde também, no mini WhatsApp (painel e tela grande):

- o **nome da conversa** de contato ou grupo sem nome salvo (na lista, sem olhinho, porque a linha já é
  um botão; no cabeçalho da conversa, com o olhinho);
- o **remetente em grupo** quando o WhatsApp só dá o número ("~+55 •• ••••-••01", com o olhinho no balão);
- a **prévia** da linha quando o remetente do grupo é só um número.

O nome acessível da linha e da conversa diz "Número escondido, termina em NN". Nos **cartões de pedido**
(`ConectorCard`), a **conta de saída** com forma de telefone (a do WhatsApp) também aparece escondida, com o
olhinho; e-mail e login aparecem como vieram. A conversa do bot continua montada e só é escondida por CSS
quando a tela grande abre, então ela declara que está fora de vista (`SuperficieAVista`), e o número
revelado num cartão sai do DOM e volta escondido.

## Quem pode mudar

Só o dono, pelo app (`donnaWhatsapp`, `donnaWhatsappSave`, `donnaWhatsappBotSave`,
`donnaWhatsappPair`, `donnaWhatsappPairCancel`, `donnaWhatsappDisconnect`,
`donnaWhatsappWacliRetry`, `donnaWhatsappChatErase`, `donnaWhatsappChatMark`,
`donnaWhatsappQueue`, `donnaWhatsappQueueDecide`, `donnaWhatsappSendDraft` e
`donnaWhatsappSend`), com a
mesma validação do editor de bot: este computador ou aparelho com Control.
`donnaWhatsappSend` e `donnaWhatsappSendDraft` são `LocalOnly` — só este computador,
nunca um aparelho com Control. As decisões da fila e a marcação de conversa passam pela
porta do dono (`Dono`). Nenhuma ferramenta do bot chega a esses ajustes nem ao número.

A leitura mostra os quatro estados separados: **plugin** ligado (e se o wacli está
instalado), **número** (com o telefone confirmado, quando houver), o que o **bot pode**
(do registro) e **envio** (o modo do bot: "peço antes" ou "sozinho"). Mudanças no
número geram o aviso `whatsappChanged` (só o ID do bot); o app lê de novo.

## Como o wacli chega

O WhatsApp usa o wacli, programa livre baixado do projeto oficial.

- **Quando:** na primeira vez que o plugin fica ligado, e sempre que ele estiver ligado
  e o programa faltar (por exemplo, depois de apagado). O Hub baixa em segundo plano;
  a tela não trava. **Plugin desligado não baixa.** Nada vem no instalador do app.
- **De onde:** só do GitHub releases do projeto oficial `openclaw/wacli`, versão fixada
  no código (`v0.25.1`). Para cada computador, o código guarda o SHA256 do pacote
  (do `checksums.txt` oficial) e o do executável.
- **Conferência:** o hash do pacote é conferido **antes** de abrir; o do executável,
  antes de pôr no lugar. Diferente = recusa e apaga; nada que falhou é executado.
  Tudo acontece numa pasta temporária dentro de `whatsapp/`, e o executável entra em
  `whatsapp/bin/wacli` (`wacli.exe` no Windows) de uma vez, por troca atômica.
  Pasta `bin` só do Hub (`0700`); o arquivo, `0755`.
- **Atualizar:** só com uma nova versão fixada no código. Se o executável em `bin/`
  tiver outro hash, o Hub põe o oficial no lugar ao conferir de novo (ao reiniciar).
- **Computadores:** Linux amd64/arm64, macOS Apple Silicon e Intel (o wacli exige
  macOS 15 ou mais novo) e Windows amd64. Em outro computador a tela mostra
  "Sem wacli oficial para este computador"; o Hub não tenta compilar.
- **macOS:** o binário oficial já vem assinado (Developer ID da OpenClaw Foundation)
  e notarizado; o Hub não reassina. Arquivo baixado pelo próprio Hub não recebe a
  marca de quarentena (`com.apple.quarantine`): ela só é posta por apps que pedem
  isso, e o Hub não pede. Conferido pelo código; ainda não num Mac real.
- **Windows:** o binário oficial **não é assinado**. O SmartScreen não entra, porque o
  Hub executa o programa direto (não é um download aberto pelo usuário), mas o
  antivírus pode alertar ou pôr em quarentena.
- **Licença:** wacli MIT, whatsmeow MPL-2.0 e libsignal GPL-3.0. O wacli roda como
  processo separado, nunca ligado ao Hub. Detalhes em `THIRD_PARTY.md`.

Quando o wacli fica pronto, o Hub sobe os números já conectados. A leitura
(`donnaWhatsapp` e as demais RPCs) traz, em `plugin.wacli`, além de `installed`, o
`state`: `off` (plugin desligado, ainda não conferido), `downloading`, `ready`,
`failed` (com `reason` curto, por exemplo `wacli_integrity: ...` ou
`wacli_download: ...`) ou `unsupported`. Depois de uma falha,
o Hub só tenta de novo quando o dono pede: `donnaWhatsappWacliRetry` (sem campos, só o
dono, como os outros ajustes). A tela mostra isso em Ajustes → Plugins (veja "Telas no app").

## Telas no app

- **Ajustes → Plugins**: o card do WhatsApp (sem segundo gerenciador). O antigo cartão [Agenda](agenda.md) foi retirado. Aviso fixo âmbar, interruptor,
  modo "Só o Berga"/"Um número por bot", os quatro estados, a lista de bots com o estado de
  cada um (pausados esmaecidos) e o link para Ajustes → Voz para texto.
- **Só com o plugin ligado:** com o plugin desligado em Ajustes → Plugins, o
  item "WhatsApp" some do menu das Configurações do bot e a seção não aparece (sem a leitura do Hub,
  também). Ligar o plugin mostra os dois na hora; desligar com a seção aberta volta para Identidade.
- **Configurações do bot → WhatsApp**: as opções da tabela acima, já com os campos de
  envio: o seletor "Como responde" com os dois modos ("Peço antes de enviar" padrão e
  "Enviar sozinho dentro dos limites"), **Limite por dia** (100/dia, 1–500), **Horário de
  silêncio** (ligado por padrão, 22h–8h, com os dois horários ajustáveis) e, no modo
  sozinho, a lista **Conversas que ela responde sozinha** (chips removíveis). "Responder"
  fica desligado e travado enquanto "Ver" está desligado; desligar "Ver" desliga
  "Responder" no mesmo pedido.
  Intervalo de 8 a 300 s ("Nunca mais rápido que 1 mensagem a cada 8 s.", grava ao soltar),
  limite de 1 a 200 por hora ("No máximo 200 por hora. Você pode baixar.") e 1 a 500 por
  dia. Plugin desligado ou bot pausado: opções esmaecidas, com a explicação.
- **Fila de envio**: cartão na mesma seção com os itens pendentes, segurados
  (com o motivo: silêncio, limite, sem conexão…), incertos (com "Enviou"/"Não saiu") e os
  enviados da última hora. Erro de leitura aparece como erro, nunca como fila vazia.
- **Número dedicado**: o pareamento avisa "recomendamos um número só do bot; no seu
  número o risco de bloqueio é seu" — recomendação, não bloqueio.
- **Número**: cada bot mostra o estado do número (sem número, conectando, conectado,
  caiu e tentando voltar, parado, precisa conectar de novo, wacli não instalado). Conectado,
  o editor diz em que computador ("Conectado neste computador: <nome>").
  "O que o bot pode" depende do número: com uma opção ligada e sem número conectado, a tela
  diz "Conecte o número para o bot ver" (o bot não recebe nada sem número).
- **wacli**: Ajustes → Plugins mostra "Baixando o wacli…", "pronto", "falhou" com o motivo
  e "Tentar de novo" (`donnaWhatsappWacliRetry`), ou "Sem wacli oficial para este computador".
  Sem wacli pronto, "Conectar" fica travado.
- **Conectar número** abre uma folha em três passos: **Riscos** (aviso fixo e a caixa
  obrigatória "Entendi os riscos"), **Conectar** (QR que se renova, com contagem, ou o código
  de 8 letras pelo número de telefone) e **Pronto** (conectado, venceu, não conectou ou
  cancelado). Conectado, o bot não pode nada até o dono ligar as opções. Fechar a folha
  durante a espera cancela o pareamento. Código e QR só aparecem na tela: não vão para log,
  analytics nem títulos. O app relê a leitura a cada aviso `whatsappChanged`.
- **Desconectar** e **Desconectar e apagar** pedem confirmação. Se o aviso ao WhatsApp falhar,
  a tela pede para tirar o aparelho em "Aparelhos conectados" no celular.
- Erro do computador (sem conexão, versão antiga sem o WhatsApp, recusa) aparece como erro com
  "Tentar de novo", nunca como lista vazia. Código: `apps/desktop/src/renderer/views/fork/whatsapp/`.

## Mini WhatsApp no painel direito

- **Onde**: aba **WhatsApp** no painel direito do bot (Sessões · Rotinas · WhatsApp), só para bot
  com número ligado a ele (conectado, conectando, desconectado, parado ou precisando reconectar).
  **Nada na barra esquerda** e nada no chat normal do bot. Não lidas: um **ponto** na aba (o número
  vai só no leitor de tela). Com a aba, cada aba tem a largura do próprio texto; faltando lugar, o
  texto corta com reticências e o ponto fica à vista.
- **Pequeno, como um celular** (332 px): estado do número, Pesquisar, pílulas **Todas · Não lidas N
  · Privadas N · Grupos** e a lista como no WhatsApp: iniciais em círculo colorido (o wacli não traz
  foto), só o nome (o número só sem nome), a prévia ("~Nome: texto" em grupo para remetente não
  salvo; "Foto", "Mensagem de voz", "Chamada de voz"… sem duração do áudio, que o wacli não guarda),
  a hora e, com não lidas, hora e bolinha em verde (`--success`). Fixadas primeiro, com alfinete.
- **Conversa**: um clique abre por cima da lista, com "‹" para voltar. Selo "conteúdo de fora", a
  faixa "Só você vê estas mensagens…", "chegou enquanto desligado". O dono
  **escreve direto** na conversa (`donnaWhatsappSend` — sai na hora pelo número
  conectado no Hub, fora da fila e dos limites, só neste computador); envio incerto
  avisa na hora. O menu ⋯ da conversa tem
  "Responder sozinho" / "Não responder sozinho" em conversas individuais. Em grupos,
  separa "Marcar grupo" / "Desmarcar grupo" (`donnaWhatsappChatMark`) de "Ligar resposta
  autônoma neste grupo" / "Desligar resposta autônoma neste grupo" (`donnaWhatsappGroupAuto`).
  **Abrir marca como lida** (`donnaWhatsappMarkRead`); a página
  de mensagens anteriores carrega ao rolar para cima.
- **Expandir / Recolher**: o botão Expandir abre a tela grande no centro (lista e conversa lado a
  lado; topo "<bot> › WhatsApp", estado, Configurações, Recolher). Recolher (ou **Esc**)
  volta ao painel **na mesma conversa** e sempre **fixo**; **⌘1** recolhe e volta à conversa do bot.
- **Estados**: sem conversas ("Nenhuma mensagem ainda"); número desconectado (lista esmaecida e a
  faixa para reconectar em Configurações do bot → WhatsApp); "Ver conversas" e "Ver grupos"
  desligados (aviso com o botão para as Configurações); erro do computador (com "Tentar de novo",
  nunca lista vazia).
- **Atualização**: o app relê a leitura, a lista e a conversa aberta a cada aviso `whatsappChanged`.
  O bot só lê a caixa quando o dono pede, como conteúdo de fora.
