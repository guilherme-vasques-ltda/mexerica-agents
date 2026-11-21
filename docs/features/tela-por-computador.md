# Mexerica — tela por bot e computador

Nas Configurações do bot, **Pode ver a tela deste computador** libera as ferramentas
de tela do host do Hub. Começa desligada para todos, inclusive bots que já tinham
Uso do computador ligado. Uso do computador e Tela remota continuam necessários.
A chave é por bot e ID estável, nunca pelo nome do computador.

Esta opção bloqueia as ferramentas de tela; não garante isolamento do terminal.
Desligar impede novos envios/capturas e invalida observações antigas mesmo se religar.
Ações já admitidas ou input em voo não são desfeitos.

Só o dono pode alterar pelo app local ou aparelho com Control. Excluir o bot,
revogar/remover o computador ou pareá-lo novamente apaga suas liberações.
Computadores antigos mostram “tela ainda indisponível neste computador”, sem chave ativa.
Para ligar uma chave, pelas Configurações ou por Permitir sempre, nenhum bot pode estar em sessão ativa de tela no host,
inclusive o próprio bot da chave. A regra vale também para o aparelho com Control.
Desligar continua permitido durante o uso da tela.

Nesse bloqueio, “sessão ativa” é a janela de 20 segundos contada do início da
última chamada de ferramenta de tela. O fim da chamada não renova essa janela.
Um clique já admitido que só execute depois dela não tem exclusão nativa
comprovada. O dono pode precisar esperar até 20 segundos após o último uso da
tela pelos bots para conseguir ligar a chave.

Como desligar continua aceito, um bot já liberado pode clicar na UI para desligar
a chave de outro bot. Isso pode interromper o uso da tela, mas não libera acesso.

## Pedido de primeira vez

Sem liberação, o Hub cria um cartão na conversa, com três escolhas. **Permitir
agora** é a escolha em destaque; **Permitir sempre** é secundária e grava a chave
junto com a resposta, na mesma transação. **Não permitir** recusa: o bot não pede
novamente no mesmo turno e fica uma hora sem criar outro pedido para esse par.
O dono ainda pode ligar nas Configurações.
Após conceder Agora, as Configurações mostram **Encerrar Agora**, que revoga a
liberação temporária sem precisar ligar a chave permanente. O aviso registra a
concessão válida neste turno, conferida pelo Hub com a origem original. A autorização é reconferida em cada uso.

Agora fica só na memória, para o mesmo bot, host, sessão, conversa e execução.
Os prazos contam pelos relógios monótono e civil: o primeiro que vencer encerra;
dormir o Mac não estende a concessão. Termina no primeiro destes acontecimentos: 20 segundos sem uso da tela, fim do
turno, outro bot assumir, 15 minutos desde a concessão (sem renovar), reinício do
Hub, revogação, recusa ou desligamento. Uma tentativa antiga não adota outra
concessão de Agora. Desligar também invalida Agora quando não existe linha permanente.

Há no máximo um cartão pendente por par. O cartão dura dez minutos; cada chamada
espera até 270 segundos sem capturar ou reservar a tela, depois informa
“aguardando o dono”. Fim do turno cancela a espera e conserva o cartão até seu
prazo. Sempre tardio liga só a chave para chamadas futuras. Agora tardio registra
resposta sem efeito, sem liberar tela. Cada resposta válida é auditada.

Permitir sempre é recusado enquanto qualquer bot está usando a tela, inclusive o
que pediu, de qualquer aparelho. Para Permitir agora durante esse uso, é necessário
Device de origem Account com Control: “abra o app conectado à sua conta”. O host
Local não concede Agora enquanto a tela está em uso. **Não permitir** é aceito de
qualquer origem válida, inclusive Local e aparelhos QR, mesmo com tela ativa.
Aparelho pareado por QR (DeviceSource::Local) nunca concede Sempre ou Agora;
a recusa de concessão mantém cartão, revisão, chave e auditoria intactos.
Nenhuma opção deste cartão aceita voz; responda pelo app.

O cartão tem só texto fixo do Hub. O motivo do bot não é gravado nem publicado e
não aparece no desktop ou nos aparelhos remotos; o argumento antigo é descartado.
Seu eventual retorno depende do port móvel com a marca de conteúdo externo.

**Limite atual:** Caller::Local pode responder Sempre,
Agora e Não, sujeito às regras acima. O app do Mac pelo túnel SSH chega como
Local; um processo dos bots no servidor também chega como Local e poderia
responder como dono. A separação do motor ainda não está disponível; não há
sinal inventado nem proteção de terminal prometida nesta versão.

A preparação inerte acrescenta o registro durável canônico da revisão e
leituras negativas antes de Sempre/Agora, do claim, de Attempt (inclusive espera),
do envio e do retorno. O schema nasce vazio e não muda as concessões do cartão.
Produção não grava hold, gesto, entrega, prova ou liberação; a caixa
continua indisponível. Uma tentativa recusada pelo hold não retoma sozinha.

Fixtures sintéticas exercitam o hold e o aborto negativo da mesma instância;
não comprovam aborto/fechamento nativo nem drenagem de Runtime. Prazo, lease,
restart, boot novo e ausência de ACK não liberam. Recusar envio novo não prova
drenar input em voo.

## Contrato do Hub

A intenção `donnaScreenAccess` recebe `botId`, `target` e `enabled`. `target` é
`host:<host_id>` do Hub atual ou `Computer.id` ativo; nomes e aliases são recusados.
Local autenticado e Device Control usam a autoridade original, reconferida junto
com bot vivo, alvo, escrita e auditoria na mesma transação. A projeção `screenAccess`
do bot é de leitura e não entra no rascunho de perfil. A UI antiga não oferece chave;
a API recusa `enabled:true` para qualquer alvo diferente do host, sem gravar linha
ou auditoria de ligar. `enabled:false` em host antigo ativo pode limpar uma linha legada.
