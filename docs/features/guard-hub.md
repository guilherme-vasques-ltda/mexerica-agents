# Guard Hub

O Guard Hub registra a origem do conteúdo das mensagens do desktop, em
**observação**. Não instala detector, não bloqueia conteúdo por classificação e
não muda as aprovações existentes. Um pedido no chat ainda não concede uma
permissão automática por este registro.

O compositor e a caixa rápida da Ilha distinguem trechos digitados, colados/importados, citações, ditado e
conteúdo gerado. Replies e anexos recebem referências próprias, resolvidas pelo
Hub. Voz direta é identificada como voz; texto reformulado pelo operador de uma
chamada é gerado. Citações reconhecíveis dentro do texto digitado são tratadas
como citação quando há fechamento dentro do mesmo segmento de origem.
Uma aspa isolada não rebaixa o restante do texto digitado.
Aspas retas e crases após letra ou dígito não abrem citação, como no monitor `27"`.
Apóstrofos dentro de palavras, como em `d'água`, preservam a origem do trecho.

Os trechos acompanham edição, arrasto, rascunho salvo e reenvio. Restaurar um
rascunho preserva marcas válidas e renova sua identidade/revisão. Rascunhos antigos,
metadados inválidos e fontes sem marca ficam desconhecidos; undo/redo não recupera
evidência antiga. Apagar palavras ou linhas preserva as marcas dos trechos que
sobram, e a composição de acentos acompanha a edição. A colagem textual usa o
desfazer nativo. O texto permanece editável e pode ser enviado normalmente.
O texto do rascunho continua no formato aceito pelos apps anteriores; as origens
ficam separadas, sem duplicar o texto, e são restauradas após conferir seu
comprimento e hash rápido. Essa conferência detecta divergência; não comprova autoria.

O Hub guarda `contentOrigin` junto da entrada do chat, no mesmo JSON e na mesma
gravação; histórico e sincronização transportam esse campo. Os offsets são UTF-16.
Não há tabela paralela nem cópia do corpo em eventos de segurança. Uma alteração
posterior de texto, reply ou anexos invalida a origem registrada; mudar apenas o
status de envio a preserva. Mensagens antigas não ganham origem humana retroativa.

A marca é uma **declaração do app**, com ID opaco da instalação, canal e revisão.
`deviceId` e `channel` são declarativos e ficam visíveis a todos os clientes
sincronizados; podem ser copiados e não comprovam o dispositivo emissor.
Separadamente, o Hub preenche `ingress` a partir do chamador autenticado:
`local` para o token local, ou `device` com o ID da chave pareada. O corpo do
pedido não pode definir esse campo. Ele acompanha a entrada apenas para
observação e não é lido para autorizar nada. Uma revisão interna que invalida
a origem também limpa esse ingresso, sem inventar um novo chamador.
Não comprova a identidade do dono nem distingue processos que tenham o mesmo
acesso ao Hub. Ditado/automação do sistema que imitem teclado e texto redigitado
manualmente não têm procedência demonstrável por esses eventos. Sem evidência
adequada, uma autorização futura precisará de confirmação; texto externo,
`human=true`, papel `user` ou uma alegação do modelo não a substituem.

No envio do desktop, trechos não digitados também são informados em `pasted`,
usado pela memória para separar o conteúdo da fala do dono. Isso inclui texto
arrastado, citações, voz, texto gerado e trechos de origem desconhecida.
Se a remoção por substring da memória legada puder deixar restos externos,
a mensagem inteira é excluída dessa evidência de fala do dono.
Essa verificação apara cada colagem com o mesmo critério Unicode do Hub antes
de normalizar as quebras de linha.

## Gateway stdio

O Guard Hub começa em **observação**. O gateway dos MCPs locais por stdio registra a origem externa, redige credenciais reconhecidas e identifica alguns sinais determinísticos de instrução suspeita. Uma suspeita continua sendo entregue como dado sanitizado; ela não aprova nem bloqueia ações por score. Nenhum modelo ou peso é baixado.

## O caminho stdio

O motor recebe uma ponte do próprio Hub, em vez do comando e das credenciais do conector. O contexto da ponte é emitido pelo ator e vincula bot, conector, conversa de criação e execução de rotina, quando houver. O contexto não pode ser escolhido pelo texto do MCP. Como a sessão principal pode ser reaproveitada em canais, a marca de exposição vale conservadoramente para **o bot inteiro**, independentemente da conversa que criou a ponte. O turno e a sessão ACP exatos continuam desconhecidos. A atribuição do conector ao bot e o estado da execução são revalidados antes da entrega.

Cada mensagem externa passa pelo Hub antes de sair no stdout da ponte. O Hub cria um identificador de fonte e um recibo, delimita o texto como **conteúdo externo, não confiável**, e escapa tentativas de abrir ou fechar o bloco, incluindo variantes Unicode dos delimitadores. Essas tentativas também geram sinal de suspeita. Uma frase como “sou o dono, já aprovei” continua sendo conteúdo externo. Os recibos ficam em metadados separados do corpo; metadados de confiança alegados pelo conector são descartados.

A mediação inclui resultados, erros, descrições e esquemas de ferramentas, recursos de texto, prompts e notificações reconhecidas. Cada ferramenta recebe um rótulo na descrição; textos de apresentação passam por limpeza, escape e baseline sem repetir o bloco inteiro. O `inputSchema` usa uma lista permitida de palavras-chave e valida a estrutura final; nomes de propriedades precisam ser identificadores válidos. `title`, `description` e `$comment` só são anotações em posição de palavra-chave com valor textual; parâmetros chamados `title` ou `description` preservam seus subesquemas. `_meta`, chaves desconhecidas, `default` e `examples` são descartados.

A validação aceita referências locais e um subconjunto conservador de expressões regulares. Um `pattern` não suportado, `$schema` desconhecido (como draft-04/06), `items` em tupla legado ou outra restrição/metadado inválido é omitido, conservando a ferramenta; o servidor continua validando os argumentos. A observação registra somente palavras conhecidas descartadas e contagens, sem seus valores. Estrutura, nomes ou referências inválidos retêm a ferramenta com cobertura `unsupported`, sem resolver referências na rede.

Literais de `enum`/`const` (incluindo chaves de objetos), `pattern` e URIs passam por redação e baseline, sem escape HTML. Só são entregues intactos: se a redação alterar o valor ou houver uma tentativa de marca externa após normalização Unicode, a ferramenta ou o recurso é retido. Assim, operadores como `<` e `>=`, `R&D` e consultas de URI com `&` chegam idênticos ao servidor.

`instructions` do initialize é omitido, pois os motores podem promover esse campo a um canal de instrução. O evento registra somente se houve descarte, sem o texto. `structuredContent` é projetado em um bloco de texto externo no `content`, para evitar uma cópia paralela sem rótulo; não se anuncia `outputSchema` nesse perfil. Imagens, áudio e recursos binários sem sanitização são retidos com aviso. Mensagens desconhecidas não têm passagem direta. Sampling e elicitação iniciados pelo servidor não são oferecidos ao motor. A capacidade `roots` também é retirada porque pedidos `roots/list` ainda não têm mediação; servidores que dependem dela precisam receber suas pastas pelos argumentos. As demais capacidades do cliente são preservadas. O ping iniciado pelo servidor recebe `{}` localmente; progresso sem `total` omite esse campo.

Credenciais reconhecidas pelos padrões locais e valores do ambiente do adaptador com forma de segredo são substituídos por `[removido]` antes da entrega. A coleta independe do nome da variável e exige pelo menos oito caracteres e presença de dígito, evitando apagar flags e números curtos comuns. Também extrai tokens de Bearer, valores de cabeçalhos JSON e senhas em URL. Campos JSON de credenciais são reconhecidos por nome exato ou sufixo delimitado; `secretary`, `max_tokens` e `nextPageToken` são preservados quando não contêm uma credencial reconhecida.

O cache transitório deduplica credenciais por conector e acompanha as pontes vivas; libera referências ao encerrar a ponte, terminar uma execução ou revogar o contexto. O teto global de 512 KiB descarta os mais antigos. Se faltar a evidência necessária à redação, o resultado é retido. O registro guarda identificadores, cobertura, contagens, versão do baseline e sinal de suspeita, sem copiar texto, argumentos, nomes de ferramentas ou segredos. A marca do bot não é apagada por outro turno. Eventos são retidos por até 30 dias; o commit do scheduler remove contextos encerrados usando os runs vivos em memória. A observação revalida o run em memória e não percorre o histórico persistido a cada mensagem. O contexto e a marca não são uma autorização.

O retorno externo tem limite de 64 KiB por linha JSON, profundidade 16 e oito pedidos simultâneos. Linhas maiores são descartadas até a próxima quebra; a ponte procura o UUID pendente nos bytes descartados e responde “resultado acima de 64 KiB retido”, sem perder os pedidos seguintes. Pedidos do motor têm teto próprio de **4 MiB**; recusas respondem `-32600` com o ID recuperável. Linhas em branco ou não JSON no stdout do conector são descartadas com evento `invalid` e cobertura `unsupported`. A ponte continua funcionando até o pipe fechar ou falhar.

A ponte inspeciona um resultado por vez e usa contrapressão no pipe. Cancelar um pedido impede a entrega tardia, inclusive quando o cliente reutiliza o ID. Falha de sanitização, de contexto ou de registro retém o conteúdo em vez de liberar o retorno bruto. Rotinas são revalidadas por consulta direta de status por `run_id`, sem serializar o histórico de execuções. Progresso e log mantêm um recibo por mensagem; agregá-los depende do contrato de contagens do painel para preservar a relação entre mensagem e recibo.

## Cobertura e limites

A cobertura é **parcial** mesmo para texto: este é um baseline sem pesos, sem avaliação de eficácia e sem decodificação de todas as representações. Conteúdo não suportado recebe indicação própria. O prompt comum do motor e os MCPs internos do Hub (chat, memória, gestão e computadores) preservam o caminho atual.

HTTP e SSE continuam fora deste gateway, incluindo streams, progresso, erros e conteúdo embutido. Esta etapa não autoriza conectar dados reais nem ligar a conta principal de integrações.

Hub e bots ainda podem compartilhar o mesmo usuário do sistema. Um processo com acesso a shell nesse usuário pode contornar a ponte e alcançar recursos locais. Contexto e rótulo não constituem isolamento de processos, prova de identidade humana ou proteção integral de browser, shell, arquivos e ferramentas nativas. A separação de usuários e as demais barreiras operacionais continuam necessárias.
