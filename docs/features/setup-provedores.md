# Contas de provedores e setup guiado

O Hub pode cadastrar contas gerenciadas de Codex e Claude, preparar o conector e conduzir a autorização humana. Login da Mexerica e login do provedor são identidades distintas. Este recurso pessoal não ativa hospedagem comercial, provisionamento ou cobrança.

Cada conta recebe identificador opaco estável, rótulo e revisão. A identidade comprovada pelo provedor e a cota ficam privadas no Hub. Contas antigas continuam com suas referências e configurações; adicionar uma conta não importa, copia ou sobrescreve o perfil antigo.

## No app

Abra **Configurações → Contas de IA** para adicionar ou gerenciar contas do Hub. No bot, use **Configurações → Motor → Conta** para escolher a conta. O painel guia a preparação e a autorização; quando o provedor oferece a alternativa, o terminal de setup fica nesse mesmo painel. Fechar o painel mantém a operação em andamento; Cancelar pede seu encerramento ao Hub.

Acima da lista de contas, **O que é cada coisa?** abre um glossário que define, em uma linha cada, conta (um acesso seu a um serviço de IA, como a assinatura do Claude), motor (o programa que roda o bot, como Claude Code ou Codex) e perfil antigo do Hub (o acesso de antes das contas gerenciadas, que continua valendo e só se reconecta pelo Hub).

Na mesma página, abaixo das contas de cada Hub, fica **Motores no Hub {nome}**: os motores (agentes ACP) que o Hub suporta, que antes ficavam na aba Agentes de Extensões. Cada linha mostra o estado que o Hub informou: Pronto, Sem login, Login não verificado, Precisa do Node.js, Não instalado ou Instala no primeiro uso. Codex e Claude Code mostram a instalação; o login deles é pelas contas da seção de cima.

Clicar abre a folha do motor, com as etapas Instalar, Node.js e Entrar e as formas de entrar que o próprio motor oferece (terminal assistido no Hub, navegador no Hub ou chave guardada só no Hub). Nada instala nem entra sozinho. A página relê os motores ao abrir, em **Atualizar motores** e ao voltar de uma folha, sempre para o Hub e a conexão atuais. A folha e o terminal assistido continuam montados numa queda ou reconexão do mesmo Hub; trocar de Hub fecha a folha. As respostas da lista continuam cercadas pela conexão e geração atuais.

### Falha de conta ou motor ao preparar um pedido

Quando o preparo de um pedido humano falha nas três tentativas por causa da conta ou do motor — conta desconectada, não confirmada, indisponível, sumida ou incompatível com o motor, motor sem login ou sem instalação, modelo não liberado, pasta inacessível —, o Hub grava no aviso e na mensagem um motivo estruturado (`reason`): código de lista fechada, `accountId` opaco e motor. A conta e o motor citados são os da configuração candidata da lane do turno, nunca os da lane anterior; o motivo nunca leva a mensagem crua do erro, e-mail, token ou caminho — valores fora do formato opaco são omitidos e código desconhecido vira `setup_failed`. Sem o erro em memória (reinício do Hub), o motivo gravado é `setup_failed`.

O app não mostra só o texto fixo: o aviso de erro da conversa vira um cartão que diz, em português simples e nos seis idiomas do app, o que houve, qual conta e qual motor o bot usa, quais contas estão conectadas e o que fazer — com o botão da ação certa e, quando ela não é abrir as contas, "Abrir Contas de IA" ao lado. A mensagem não enviada mostra a ação certa ao lado de "Não enviada", com a frase inteira na dica. O texto gravado no Hub continua o fixo de sempre; muda só a exibição. App ou Hub antigo cai no texto fixo — e, com Hub antigo, o aviso de preparo ainda ganha o botão "Abrir Contas de IA".

A conta citada é resolvida pela lista de Contas de IA: fora da lista, a frase admite que a conta sumiu em vez de inventar estado; lista não carregada, o cartão diz que não confirmou quais contas estão conectadas. A linha "Contas conectadas: …" nomeia as contas conectadas (até quatro, depois "+N") ou admite o desconhecido. Uma linha "Conta do bot: …" repete conta, motor e estado quando a frase principal não a nomeou; sem `accountId` — salvo `folder_unavailable` —, avisa que o bot usa o acesso do Hub.

| Motivo (`reason.code`) | O que o cartão diz | Botão principal |
| --- | --- | --- |
| `account_disconnected`, `account_unavailable` | A conta do bot está desconectada, não confirmada, indisponível ou conectada sem uso | **Reconectar** |
| `account_not_found`, `account_incompatible`, `engine_unavailable`, `engine_choice_execution_mismatch` | A conta sumiu, não combina com o motor, a conta da conversa diverge da do bot ou não foi possível ler motor e conta | **Trocar a conta do bot** |
| `cli_unavailable` | O programa do motor não está pronto no Hub para a conta | **Reconectar** |
| `model_unavailable`, `model_option_unavailable` | O modelo ou uma opção dele não está liberado pela conta | **Trocar a conta ou o modelo** |
| `engine_not_ready` em motor gerenciado | O motor do Hub precisa de login ou instalação e o bot não tem conta própria | **Conectar** |
| `engine_not_ready` em motor fora dos gerenciados (ex.: `omp`) | O motor de IA escolhido não está disponível neste Hub | **Trocar o motor do bot** |
| `folder_unavailable` | A pasta do bot não existe mais ou não pode ser aberta | **Escolher pasta** |
| `setup_in_progress`, `space_account_selection_required`, `routing_selection_unavailable`, `setup_failed` e demais | Login em andamento, espaço sem conta, conta a reconfirmar ou falha genérica | só **Abrir Contas de IA** |

Os botões de conta abrem **Configurações → Contas de IA**; os de troca abrem as Configurações do bot na aba **Motor** ("Trocar a conta do bot", "Trocar a conta ou o modelo", "Trocar o motor do bot") ou na aba **Comportamento** ("Escolher pasta"). Quando a conta citada não está na lista ou o motivo é `account_not_found`, a ação principal já é a troca; sem conta resolvida, fica só "Abrir Contas de IA".

A mesma conta aparece em linha própria — "Conta: {conta} · {motor} · {estado}", "sem conta (usa o acesso do Hub)" ou "não consegui confirmar" — na barra de contexto do compositor e na aba Motor das Configurações do bot, só em motor gerenciado (Codex/Claude). A conta da conversa vale quando a sessão a traz; senão, a escolha do perfil. A linha abre Contas de IA (nas configurações do bot, o painel de contas) e marca atenção quando a conta não está conectada ou não pôde ser confirmada.

Os pontos de recuperação se repetem fora da conversa: o nascimento de um Space sem conta e a ativação de principal de Space sem opções de conta mostram **Abrir Contas de IA**; no editor de funções do bot de fundo, os motivos de conta (`account_missing`, `account_choice_required`, `account_disconnected`, `billing_unknown`, `api_not_enabled`, `subscription_unverified`, `accounts_unavailable`) ganham o mesmo botão; e o aviso de modelo indisponível no seletor da conversa nomeia a conta quando ela está no cadastro do Hub.

## Consulta de limites Claude

O painel geral consulta o relatório local `/usage` do Claude Code instalado,
com a configuração MCP vazia e estrita. Essa consulta não carrega os MCPs
configurados, não cria turno de conversa, não persiste sessão e não chama o
modelo. Ela usa o login já salvo e o ambiente filtrado da Mexerica, incluindo a
configuração do slot Claude quando disponível. Se o cofre estiver travado,
consulta somente o perfil já existente com o ambiente herdado filtrado. Falha
ou timeout não abre login, não cria conta e não inicia conversa.

Consultar uso, verificar login e enviar uma mensagem ao modelo são operações
diferentes. O relatório global não comprova a cota de uma conta selecionada no
bot. `donnaEngineAccountUsage` continua exigindo proveniência individual; este
porte não habilita cota individual Claude nem acrescenta ferramenta ou skill.

## Consulta global de limites Codex

O painel por computador/provedor consulta os limites atuais pela API da CLI
Codex (`account/rateLimits/read`), sem iniciar turno, prompt ou sessão de modelo.
Usa o ambiente filtrado da Mexerica e o perfil global existente: CODEX_HOME quando
definido, ou `~/.codex` quando ausente. Perfil explícito inválido não recai em
outro login. Não usa arquivos antigos de rollout nem a conta do bot principal.

Cada leitura completa substitui as janelas Codex; janelas omitidas ou com reset
já vencido saem do painel. Falha, desautenticação, timeout, CLI ausente ou leitura
vazia removem a leitura anterior: indisponível não significa 0%. Um zero
explicitamente informado pela API continua válido. Claude conserva sua coleta
e seus eventos parciais. A consulta individual de contas mantém suas próprias
provas e autoridade; o painel global não comprova a cota de uma conta gerenciada
nem concede permissão de gestão. Não há ferramenta ou skill nova.

## Fluxo e recuperação

1. Cadastrar uma conta CLI cria um perfil privado vazio no Hub. Cadastrar API prepara uma referência própria no cofre.
2. Preparar instala somente o artefato oficial fixado para a plataforma. Uma instalação compatível existente pode ser reutilizada. A operação não autentica a conta: ao terminar a preparação, o cadastro ainda pode estar não verificado.
3. Entrar usa o fluxo de autorização do provedor. Codex usa device code pelo App Server apenas para autenticação e consultas. Claude usa `auth login` em processo próprio e depois `auth status --json`. Consentimento, MFA e outros passos humanos continuam no provedor.
4. Só a consulta final com identidade e modalidade reconhecidas conecta a conta CLI. Um processo que termina com sucesso, uma página de retorno ou texto do terminal não comprovam login. Capacidades desconhecidas falham sem trocar de mecanismo silenciosamente.
5. Selecionar a conta no editor aplica o perfil no próximo turno. Motor, modelo e opções são validados pelo Hub. Uma seleção indisponível falha; não recai na configuração global nem em API paga.
   Na criação ([Novo bot](criar-bot.md), passo Motor), a mesma seleção vai no `createBot` como `forkEditor.engineAccountId`, com a mesma autoridade `engine.accounts` capturada na entrada e rechecada antes do commit; a tela só envia a intenção, nunca credencial, e mostra a recusa pelo código.

O prazo original da operação cobre também a busca/checagem do executável e a descoberta posterior do catálogo. Retry de início, leitura e eventos do worker aplicam a mesma expiração: prazo vencido remove desafio/apresentação, encerra processos de setup e impede promoção, mesmo sem a interface consultar progresso.

Depois da prova de identidade e catálogo do Codex, o Hub fecha a entrada do App Server e espera até cinco segundos por sua saída normal e bem-sucedida. Isso permite ao CLI remover seus próprios links temporários antes da conferência do perfil. Saída com erro, falha de espera ou timeout recusa a promoção com `auth_shutdown_failed`; o encerramento forçado continua como limpeza de falha/cancelamento. Links remanescentes ou inesperados continuam recusados, e o perfil anterior permanece preservado. A conta só aparece conectada depois dessa finalização e das guardas de autoridade; sucesso no navegador não substitui essas etapas nem provoca outro login automático.

Reconectar sempre usa perfil vazio. A referência anterior só muda depois de comprovar a mesma identidade e com os bots ociosos. Outra identidade exige cadastrar outra conta. Cancelar, expirar ou receber uma resposta atrasada não promove a tentativa. Reiniciar o Hub encerra o acompanhamento da operação; releia a conta e inicie outra tentativa, sem login relançado automaticamente.

Desconectar bloqueia novos usos da conta. Remover recusa contas vinculadas a bots. Essas ações não anunciam revogação no provedor: o registro local e a autorização do fornecedor são coisas distintas. Nenhuma ação executa logout em perfil legado. Exceção (Excluir na tela Contas de IA): `donnaEngineAccountRemove` aceita um perfil legado só se já estiver desconectado, for perfil CLI do próprio computador (nunca cofre nem cópia migrada `accounts/<id>/legacy`), sem bots e na revisão mostrada; tira apenas o registro da lista (nenhum arquivo, login ou segredo é apagado) e, se o login voltar, a sondagem o lista de novo. Conectado ou outro tipo continua `legacy_preserved`; desconectar legado continua recusado. Na tela, **Testar** é `donnaEngineAccounts {refresh:true}` (consulta do CLI sem mensagem a modelo e sem custo), e Excluir de conta em uso por bots só explica: o Hub segue recusando com `account_in_use`.

## Terminal alternativo

O terminal alternativo se anexa ao mesmo processo Claude de autorização. Ele não inicia shell nem recebe comandos, caminhos ou instaladores. A apresentação contém apenas mensagens permitidas pelo Hub; dados brutos de stdout, códigos de controle, clipboard e links de terminal não são repassados.

A entrada aceita somente um código de autorização após um desafio oficial válido. Redimensionamento é limitado. A preparação do instalador e o device code do Codex não aceitam stdin pelo terminal. Cancelamento e expiração encerram o grupo de processos e removem desafio e apresentação da resposta.

## Contrato do cliente

Os métodos usam o transporte autenticado existente, sem nova operação SSH no renderer:

- `donnaEngineAccess`: `{}` → `{capability:"engine.accounts", canManage:boolean}`; observação transitória do Hub, sem conceder autoridade ao cliente.
- `donnaEngineAccounts`: contas conhecidas, perfis gerenciados e bots vinculados; mesma capacidade, inclusive para atualizar a sondagem.
- `donnaEngineAccountCreate/Rename/Disconnect/Remove`: intenções explícitas com revisão; criação e início usam identificador de pedido idempotente.
- `donnaEngineAccountModelVisibility`: `{engineAccountId, expectedRevision, modelId, hidden}` oculta ou mostra um modelo da conta e devolve a conta atualizada. Um gesto = um modelo, idempotente; `modelId` não precisa constar do catálogo.
- `donnaEngineSetupStart`: uma operação por conta, com instalação ou login.
- `donnaEngineSetupAction`: leitura protegida, cancelamento e interação restrita vinculados ao ID e à geração da operação.
- `donnaEngineAccountUsage`: cota individual apenas com proveniência comprovada. A ausência de prova devolve indisponível; cota global não substitui a individual.
- `setAgentEnv` com `engineAccountId` e `expectedRevision`: chave API explícita, somente na variável permitida pelo provedor e em slot cifrado próprio. O formato legado sem conta permanece disponível.
- `donnaBotEditorSave`: versão esperada e patch parcial com `backend`, `model`, `modelOptions`, `fastMode` e `forkEditor.engineAccountId`, além dos campos de perfil existentes.
- `sessionEngineSet`: `{botId, sessionId, expectedRevision, engine:{accountId, backend, model, modelOptions, fastMode}}` troca a seleção de UMA conversa do Pessoal (`sessionId: null` = principal; nunca o padrão do bot). Mesma capacidade `engine.accounts` (`engine_owner_only` sem ela), guarda transacional `Authority::with_db` e validação canônica (`validate_selection_in`) do cadastro na mesma transação; revisão divergente → `session_revision_conflict`. Sem conta gerenciada, só a principal muda modelo/opções/velocidade no mesmo motor, pelas regras do editor. O padrão salvo em `donnaBotEditorSave` vale apenas para conversas novas. Uma conta usada pela seleção de uma conversa (efetiva ou próxima) aparece em `botIds` e conta como em uso: remover recusa `account_in_use` (releitura final na mesma transação que retira o cadastro) e setup/desconexão/remoção recusam `account_busy` enquanto um desses bots tem turno preparado, em execução, aguardando o limite ou pedindo resposta. A checagem final de ocupação e a escrita de desconectar, remover e iniciar ou concluir o setup ficam sob a mesma trava de runtime que o preparo de turno usa: o turno que começou antes faz a mudança recusar sem alterar o cadastro; o que tenta começar depois espera a escrita e encontra a conta desconectada (lançamento, ativação e permit recusam) ou o setup ativo (`setup_in_progress`). O início do setup só aceita a tentativa depois dessa checagem: recusa por ocupação, capacidade ou falha da primeira gravação não deixa operação nem grava a tentativa, e o mesmo `clientRequestId` pode ser repetido quando o bot ficar livre, inclusive após reiniciar. Tentativa aceita continua consumindo o ID mesmo se for cancelada, expirar ou falhar depois; se o processo cair entre a gravação e a publicação da operação, o pedido segue o comportamento já existente (`operation_expired`). Seleção gerenciada sem modelo explícito não aceita opções de modelo (`model_option_unavailable`). Isso vale também para seleções já salvas antes desta regra (padrão do bot ou retrato herdado por conversas): não há migração silenciosa que invente modelo ou apague opções; a execução e a troca recusam até o dono escolher um modelo ou limpar as opções.

O catálogo Claude confirmado possui prova privada vinculada à conta, identidade, perfil, modalidade e revisão. Uma sondagem somente de autenticação não equivale a um catálogo vazio: ela preserva a prova ainda válida. Um catálogo explicitamente consultado e vazio a substitui. Logoff, divergência de identidade/perfil ou revisão incompatível retiram a prova; registros sem prova explícita precisam de consulta nova. Renomear o rótulo conserva a mesma origem, atualizando sua revisão.

Ocultar modelo é preferência global do dono, guardada no Hub e valendo para todos os aparelhos. O DTO de `donnaEngineAccounts` traz `hiddenModelIds` por conta e `models` segue completo: o app filtra a listagem e a escolha nova, e o Hub não esconde o modelo de quem já o usa — bot, conversa ou função de fundo com o modelo oculto continuam funcionando, sem troca nem interrupção (a revisão da conta não muda, pois o carimbo das sessões a inclui). Um id que some do catálogo permanece guardado sem erro; remover a conta apaga os ocultos dela. Só contas gerenciadas são mutáveis (`legacy_preserved` nas legadas, como nas demais alterações). Hubs antigos não enviam `hiddenModelIds`: o app trata a ausência como recurso não suportado.

A capacidade própria `engine.accounts` permite gestão local autenticada e pelo Device exato da posse ACS4 vigente: chave, grant, recibo, geração, ativação assinada, lease e dono atual precisam corresponder. `owner.devices` sozinho não concede gestão de IA, shell, cofre ou administração geral. Aparelhos pareados, Bootstrap, outros Devices, grant substituído/revogado, sincronização indisponível e lease/ativação vencidos recebem `remote_setup_unavailable`. Cada pedido consulta a autoridade atual; uma conexão aberta não conserva autorização vencida.

Pela nuvem, use as contas CLI de Codex/Claude no painel existente. O link/código oficial abre no navegador deste Mac; senha, consentimento e MFA ficam no provedor. O início devolve ID/geração imediatamente, e a UI acompanha com chamadas curtas; não usa `agentAuthenticate` de 660 segundos. O atalho de entrada da conversa permite conectar e escolher a conta para esse bot, sem enviar mensagem automaticamente. Fora da conversa, selecione a conta em Configurações do bot → Motor → Conta. A leitura `teamSettings` fica disponível para mostrar a principal; `setTeamSettings` continua local. No Berga fixo, motor e conta de IA mudam só pelo app do dono, com a mesma guarda transacional; bots, rotinas e chamadas sem o app do dono são recusados com `donna_fixed` ([contrato](donna-fixa.md)).

Cada pedido conserva a autoridade original durante esperas, inclusive as filas de configuração dos bots e de mutações de contas. Depois de obter a trava, revalida o mesmo vínculo; antes da escrita, a validação ACS4 e a mudança compartilham a transação do banco e a trava do proprietário Cloud. Revogação e troca de dono são ordenadas com essa escrita; lease/ativação que vencem dentro da transação impedem o commit. Isso vale para createBot, updateBot, editor, cadastro/renomeação/desconexão/remoção, início/promoção do setup e gravação de catálogo. Remoção de slot e de conta ficam na mesma transação. Uma capacidade nova não reaproveita o pedido antigo.

A autenticação do transporte conserva a chave, origem, concessão, escopos, dono e prazo daquele pedido. Revogar e autorizar de novo o mesmo aparelho não recupera um pedido antigo, mesmo com os mesmos dados. Sair e voltar ao mesmo dono também encerra os pedidos anteriores. Perder a sincronização encerra pedidos Account em fila; voltar a ficar sincronizado não os recupera. Somente uma nova solicitação da mesma concessão vigente recebe a época atual de sincronização. A renovação da mesma concessão permite novas solicitações, mas não estende o prazo de uma solicitação já em fila. Se ela for recusada, confirme o acesso atual no app e faça uma nova solicitação; não repita login automaticamente.

O controle comum do aparelho não consulta nem concede `engine.accounts`. A gestão de contas compõe essa autorização original com a posse ACS4 e a ativação assinada já exigidas, para o mesmo sujeito e a mesma solicitação. Aparelho pareado localmente continua sendo Device; somente o transporte loopback autenticado representa Caller Local. Nenhum RPC administrativo local é aberto ao bot. O contrato interno está em [Autoridade original de transporte](autoridade-de-transporte.md).

Consultas de uso e de catálogo revalidam o vínculo capturado antes de entregar a resposta. Troca de dono, geração/recibo/grant, revogação ou expiração durante a consulta recusa o retorno privado, mesmo que o canal do Device ainda esteja aberto. Releia pelo app após confirmar a conexão atual; o Hub não entrega a resposta antiga ao dono anterior.

A operação fica vinculada ao principal, grant, geração e recibo originais. Cada leitura/ação revalida o vínculo; um monitor cancela quando a autorização deixa de valer, mesmo com o painel fechado. O resultado só é promovido após nova validação. Trocar lease/ativação conservando a mesma posse permite continuar. Pedido idêntico retorna a operação existente; corpo/revisão divergentes conflitam. Outro vínculo não pode ler ou reaproveitar seu ID. Cancelamento remove desafio e perfil provisório sem substituir o perfil anterior.

O cadastro de API associado a uma conta (`setAgentEnv` com `engineAccountId`) continua Local; a capacidade desta etapa só oferece login oficial pela nuvem. Rotas legadas de `setAgentEnv` sem conta e `credential*` também são acessíveis ao Control existente, inclusive no iOS; isso não concede `engine.accounts`. A lacuna legada permanece registrada para tratamento posterior, sem fechar ACS4/iOS incidentalmente. Cerimônias legadas de instalação/login Claude/Codex e aliases continuam Local, sem desvio pelo terminal antigo. Entrada, redimensionamento, fechamento e saída do terminal legado verificam a mesma autoridade. Nos demais provedores, o aparelho com Control conserva somente os terminais que criou. Isso não ativa serviço comercial nem autoriza custo ou dados adicionais.

As futuras APIs próprias de contas/chaves precisam conservar a Authority
original e suas guardas; a origem antiga não herdará o acesso genérico ao
cofre/env. Chaves e contas hoje gravadas pelo SSH como Caller::Local permanecem
nesse caminho até prova de paridade. Nenhum segredo passa por chat/log/telemetria
ou permanece no aparelho. A configuração real continua fechada; não há nova ação
de setup disponível ao usuário nem RPC de gestão oferecido ao bot nesta etapa.

URL/código de autorização e apresentação do terminal existem somente na leitura protegida da operação, em memória transitória. Não entram no mirror, SSE geral, chat, analytics ou logs. Os DTOs de conta excluem e-mail, identidade de cota, caminhos, comandos e referências de cofre.

## Instalador e limites

O catálogo fixa Codex 0.164.0 e Claude Code 2.1.309 para Linux x64/ARM64. O download usa HTTPS e SHA-256 fixado, limites de tamanho/tempo, trava por motor, staging privado e promoção por rename. Não executa script de instalação, shell, sudo, npm ou entrada do usuário. Uma falha conserva a instalação anterior; a substituição de conector é recusada durante turnos ativos.

Fontes de integridade: [release oficial Codex 0.164.0](https://github.com/openai/codex/releases/tag/rust-v0.164.0) e [manifesto oficial Claude 2.1.309](https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/2.1.309/manifest.json). Protocolos: [Codex App Server](https://learn.chatgpt.com/docs/app-server), [autenticação Codex](https://learn.chatgpt.com/docs/auth), [autenticação Claude](https://code.claude.com/docs/en/authentication).

Perfis privados separam seleção e custódia, com diretórios 0700 e arquivos 0600. Eles não constituem sandbox entre processos do mesmo usuário Unix e não resolvem a separação de usuários/tenants. As provas desta implementação usam processos próprios de fixture; não comprovam login real, elegibilidade comercial, implantação ou isolamento em produção.

## Bot de fundo e autorização por Space

Configurações → Bot de fundo usa a seleção por função já existente:
`functionModels`, `saveFunctionModel` e `checkFunctionModel`. Essas três operações
continuam locais. Conferir uma escolha não inicia um trabalho e não comprova
identidade, cota ou capacidade de execução naquele instante. Uma recusa ou
indisponibilidade não troca silenciosamente de conta, modelo ou provedor. Única exceção: o [fallback entre contas do mesmo motor](contas-de-motor.md#fallback-entre-contas-do-mesmo-motor), só pela lista de reserva que o dono montou e sempre com aviso na conversa.

`backgroundSpaceSelections` e `saveBackgroundSpaceSelection` administram somente
uma referência à seleção Memória, com revisões da função, conta e Space. Salvar
exige as revisões esperadas; remover não precisa inventar referência de conta
que já deixou de existir. Mudança incompatível torna a autorização desatualizada.
Não se duplica catálogo, credencial nem seleção no Space. A leitura protegida
do vínculo projeta somente o rótulo da conta e o modelo selecionado para o dono
remoto conferir a intenção; não libera o documento global ou o catálogo completo.

Essa gestão exige Local ou a capacidade `engine.accounts` da posse
original, com revalidação na transação sob a guarda proprietário Cloud → DB.
Device com Control ou `owner.devices` não basta. Nenhuma dessas operações é
oferecida como MCP ao bot. Trabalhos conservam a autoridade original através de
fila/await e a revalidam antes de liberar conteúdo e antes do efeito; não resolvem
uma concessão nova para aproveitar o pedido antigo.

**Autorização cadastrada não libera o executor.** O executor mantém a
cerca Pessoal sem contexto. Outros Spaces aguardam prova da conta do próprio
processo que envia o conteúdo, inclusive na criação da sessão. Consulta oficial
separada, hash de perfil e TTL não bastam; ausência de prova não recai na conta
global. Sem assinatura Claude elegível ou sem prova compatível no Space, memória
automática fica indisponível e o pedido permanece no histórico, sujeito aos
limites da fila de referências. A UI mostra somente estados devolvidos pelo Hub.

O histórico do executor é Local, limitado a 50 registros. O status é
obrigatório; modelo efetivo e etapa podem não ter sido informados. Uso opcional
é só contagem de tokens, sem preço ou motivo detalhado inventado. Se a leitura
estiver indisponível, o painel informa esse estado sem manter registros antigos
nem simular atividade.

## Preferências de roteamento

Em **Configurações do bot → Motor**, [Roteamento de modelos](roteamento-de-modelos.md)
guarda Fixo/Auto desejado, guias e reservas em ordem. **Auto e reservas continuam
indisponíveis; salvar só guarda a preferência e o bot continua no Fixo.** A
conferência estática lê conta/modelo/opções do catálogo exato, sem chamar modelo,
consultar rede, abrir cofre ou comprovar cota. O cadastro global do dono não prova
filiação conta/Space/pagador. Fora do Pessoal, contas não são listadas por essa tela
sem prova; configuração e execução legítimas existentes são preservadas.

A Authority original de `engine.accounts` atravessa a fila; posse, lease e ativação
são revalidados na guarda de gravação e antes da resposta privada. Control e
`owner.devices` não concedem essa capacidade. API/cofre continuam locais; não há
nova ferramenta de bot ou nova concessão. O teste real manual fica para uma
entrega posterior, sem botão ou chamada nesta etapa.

**Sessões extras.** Na primeira mensagem de uma extra, o Hub fixa nela a conta e
o modelo do Fixo vigentes, com recibo do Roteamento e carimbo da própria sessão. Depois,
trocar a conta ou o modelo do bot vale para a principal e para sessões ainda sem
mensagem, não para as já fixadas; se a conta fixada deixar de valer, a sessão falha
fechado, sem cair em outra conta. A extra precisa de conta gerenciada com modelo
explícito; sem isso, o envio para ela é recusado antes de gravar; a principal segue o Fixo atual, e a conversa de Space, a própria escolha. Isso não amplia `engine.accounts` nem cria controle para bots.

## Migração administrativa Linux

Na migração da instalação de teste para o usuário separado do motor, a cópia dos
perfis CLI já referidos pela Mexerica é obrigatória, com originais e
hashes guardados para volta. Isso não altera o cadastro comum de uma conta:
nenhum perfil pessoal é descoberto ou importado por inferência. O instalador
consome o inventário da migração e chama `migrar-raizes` como Hub para reescrever as
referências; não promove contas nem modifica as guardas de autoridade. A migração exige
`host.lock`: o instalador mantém a trava e a chamada direta recusa se estiver
ocupada após um segundo. Se a CLI recusar
o perfil copiado, o dono refaz o login pelo app. Senha, código e token não passam
pelo chat. Veja o [guia da instalação e volta](../guides/instalacao-hub-linux.md).
A validação com contas reais e sentinela do cofre fica para etapa posterior.
### Perfis com usuário de motor separado

Quando `/etc/donnabot/motor.conf` existe, perfis geridos ficam sob a raiz privada
do motor. O Hub cria, valida, protege e remove esses perfis pelo ajudante, após a
troca de UID; o diretório de execução também é aberto depois dessa troca. O app
continua sendo o canal de login oficial. Não peça credenciais no chat.

A migração offline, chamada pelo instalador com o serviço parado, preserva IDs e
reescreve `Metadata.profile`, referências de lançamento e o binding do catálogo
na mesma transação, com cópia dos valores anteriores. Repetir a migração concluída
não reescreve nem substitui o backup. As guardas de autoridade e a autoridade capturada
não mudam; caminho Unix não autoriza operação de conta. Instalação e aceite com
login real pertencem às etapas seguintes, sem alteração do servidor do dono nesta etapa.

Contas CLI legadas copiadas para `accounts/<id>/legacy` são sondadas nesse destino,
com HOME da geração e conferência da identidade nativa antes de manter a conta
conectada. Uma identidade diferente não adota o perfil de outra conta. Contas
históricas que usavam a mesma origem recebem destinos próprios na migração; isso
não cria novos logins nem muda as guardas de acesso.

### Instalação e descoberta como motor

Com o modo separado configurado, o Hub delega a instalação e atualização dos
adaptadores ACP, Node/npm e CLIs do setup ao próprio host iniciado pelo ajudante
como motor. Downloads, verificações de integridade, extração, scripts npm e
publicação usam a raiz privada do motor. Cache e prefixo npm ficam nessa raiz;
o ambiente do instalador não herda credenciais nem configurações npm do Hub.
Os hashes fixados e as verificações existentes continuam obrigatórios.

Descoberta e estado são consultados como motor. Antes de publicar o resultado,
o Hub pede a conferência do arquivo na raiz do motor por descritores sem links;
não abre pastas privadas do motor nem executa o binário como Hub. Uma referência
`LaunchReference.executable` ainda na instalação privada do Hub é recusada,
mesmo quando o perfil já foi migrado. Migrar o perfil não move o binário: prepare
o conector novamente para publicar uma referência à instalação do motor. A
reinstalação conserva as guardas e a autoridade original da operação.
Sem configuração do modo separado, o comportamento anterior é preservado.

### Motores não gerenciados em Hub remoto

Desktop e web usam o mesmo **terminal de login** da AgentSheet, implementado por
`agentSetup` (passo `login`) e `SetupTerminalView`. É o PTY existente do Hub,
endurecido para login: o app não escolhe programa, argumentos, ambiente ou pasta.
A gestão exige a posse original ACS4 e `engine.accounts`; só `Control` não basta.
Contas gerenciadas de Claude/Codex continuam no fluxo de **Contas de IA**.

| Motor | Comando iniciado pelo Hub | Conferência automática sem prompt |
| --- | --- | --- |
| Cursor | `cursor-agent login`, com `NO_OPEN_BROWSER=1` | `cursor-agent status` no mesmo binário e perfil |
| Grok | `grok login --device-auth` | `grok models`, sucesso e declaração explícita de login |

A lista do terminal remoto é fechada: apenas Cursor e Grok.
OpenCode, Cline, Kimi, Pi, Hermes e todos os outros motores ficam indisponíveis
nesse terminal até terem prova própria contra escape. Não há fallback para o
comando de login do cadastro nem para shell; use chave no cofre ou código de
aparelho pelo fluxo próprio, quando disponível. Falha, resposta desconhecida e
ausência de consulta nunca viram login confirmado.

**OpenCode Go:** o caminho simples permanece **Chave OpenCode Go**. Salvar ou
remover remotamente usa o método dedicado `engineGoKeySave`, que aceita só
`OPENCODE_API_KEY` no slot cifrado do OpenCode, sob `engine.accounts`/ACS4. Outros campos de ambiente e `engineAccountId` são
recusados, assim como `spaceId`: o slot vale para o Hub inteiro. `setAgentEnv`
continua exclusivamente local. As duas gravações usam a mesma trava para evitar
perda de atualização. A chave vai só ao cofre e ao ambiente do motor. Guardar não comprova
assinatura, saldo nem acesso a modelo; escolha um modelo `opencode-go/` no bot.

O terminal permite setas e Enter nos menus e código colado depois de desafio com
URL HTTPS de host oficial. Rejeita Ctrl-C/Z/D, comandos, multiline
e escapes de shell. O texto é acumulado em memória no Hub até Enter; depois do
código enviado, a saída nativa é suprimida para impedir eco do segredo. Nos fluxos de link do Cursor e do Grok, a entrada fecha assim que o desafio oficial aparece; a autenticação continua no navegador. O Hub
mostra somente avisos fixos de espera ou recusa; após uma recusa, inicie outro login. Não há
scrollback, replay, histórico, mirror, analytics ou saída no chat. Ao perder a
conexão, inicie outra tentativa; não se recupera o conteúdo anterior. Apenas o
titular original lê ou digita, com reconferência a cada entrada/saída. A saída apresentada
ao titular perde OSC, DCS, SOS, PM e APC, em 7 bits (`ESC ]`, `ESC P`, `ESC X`, `ESC ^`, `ESC _`) e em 8 bits (U+0090, U+0098, U+009D–U+009F, que chegam em UTF-8 como `C2 xx`), até BEL ou ST.
O terminal só abre links dos motores oferecidos nele (Cursor e Grok).

O Hub consulta o login após três segundos, dobrando o intervalo até 60 segundos; após o envio do código ou confirmação, consulta imediatamente e a cada segundo. Cada consulta é limitada a cinco segundos. A entrada permanece fechada após o envio. Fecha a sessão após
confirmação, fim do CLI, cancelamento, revogação ou
prazo máximo de nove minutos. Cada tentativa cria perfil provisório com identificador e prazo. Na próxima tentativa do mesmo motor e Space, o Hub limpa somente operações registradas com marcador vencido ou cancelado, sem seguir links; perfis antigos sem marcador são preservados e geram aviso. O cancelamento remove apenas o perfil da própria tentativa após encerrar seus processos. Link simbólico dentro de um perfil provisório é apagado como entrada, sem seguir o destino; a contagem de perfis sem marcador só avisa e nunca derruba o login. Perfis promovidos são preservados, exceto o que acaba de ser substituído: confirmada a troca, o Hub apaga a pasta antiga (credencial da conta anterior) sem seguir links; o registro da troca é gravado na mesma transação e, se a remoção falhar, a próxima tentativa do mesmo motor e Space repete a limpeza. O resultado confirmado retoma os cartões pendentes
sob guardas próprias de commit e admissão do reenvio. Os timeouts de início,
descoberta e salvamento no app cabem no limite de 120 segundos do relay.

Cada tentativa cria um perfil provisório isolado por Space, com HOME/XDG próprios
em pasta 0700 e processo direto com ambiente mínimo (PATH, caminhos do perfil e
opções fixas; EDITOR/VISUAL/PAGER apontam para `/usr/bin/false`). Só a conferência
positiva e o commit sob o vínculo original vigente promovem essa pasta a perfil
ativo. Cancelamento, revogação, troca de posse ou expiração apagam a provisória
depois de encerrar os processos e conservam a conta anterior. Começar o login não
muda a conta nem a sessão dos bots. Só os bots que passam a usar o perfil promovido
mudam de sessão; carimbos sem perfil mantêm o formato anterior. O perfil
legado permanece no disco; ele não é copiado para o novo login. Seleção de várias
contas para esses motores ainda não faz parte deste contrato. O cofre Go usa o
slot do motor já existente. Claude/Codex preservam seus perfis gerenciados.

**Antigravity:** a integração no app está suspensa. Para uso pessoal, use o
[CLI oficial em modo ACP](https://antigravity.google/docs/ide/extensions/acp/)
em um editor interativo. Faça login diretamente no CLI com `agy acp --login`
ou em uma sessão interativa do `agy`. O Hub não inicia o CLI nem gerencia a conta.

Referências e trechos adaptados: [Tucano, login-terminal.ts](https://github.com/eduwxyz/tucano/blob/9474545f0415f4bb2a439e1a7727912254b69972/src/core/login-terminal.ts),
[autenticação Cursor](https://cursor.com/docs/cli/reference/authentication),
[autenticação Grok](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md)
e [OpenCode](https://opencode.ai/docs/cli/providers/). Procedência em
`THIRD_PARTY.md`. Os testes com processos fictícios e Hub real não substituem o
aceite com as contas e os binários instalados em desktop/web.

Para os motores compatíveis, na API local `agentAuth` mantém a descoberta ACP dos métodos e campos de chave,
`agentAuthenticate` mantém a autenticação ACP e `setAgentEnv` mantém o cofre local.
Uma confirmação positiva nesses métodos retoma as conversas que esperavam login.
A projeção restrita de estado do perfil assistido aplica-se ao chamador remoto.

A matriz de escape cobre cada motor permitido, Ctrl-C/D/Z, Esc e editor/pager.
Os testes de ciclo cobrem concessão renovada ao mesmo aparelho, troca de posse,
outro aparelho com Control, promoção, cancelamento e expiração. O teste
`cli_real_por_motor_recusa_escape_controles_editor_e_pager` fica `#[ignore]`
para execução explícita na máquina com CLIs reais, em perfis descartáveis e sem
reutilizar credenciais. Ele não usa cota de modelo nem envia código de login.


### Sondagem dos CLIs sem autenticação

Os comandos fixos de login são `cursor-agent login` (Cursor 2026.11.13),
e `grok login --device-auth` (Grok 1.0.77). Antes de autenticar, os controles e
as tentativas de escape ficaram na espera ou no eco, sem chegar a um shell. A
sondagem sem login não comprova o comportamento autenticado. Nenhum código de
aparelho foi registrado.
