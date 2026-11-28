# Mexerica — segurança dos programas de IA

Você não precisa alterar suas configurações nesta etapa. A Mexerica mantém as
proteções e os pedidos de confirmação atuais. As novas regras de aprovação ainda
estão sendo preparadas; esta atualização não as coloca em uso.

Algumas ações dos programas de IA podem acontecer sem passar pela Mexerica.
As regras em preparação não garantem proteção para essas ações.

A seguir estão os detalhes técnicos dessas proteções, para quem desenvolve ou
mantém a Mexerica.

## Referência para desenvolvimento

O chão próprio substitui somente o núcleo
preparatório. Não ativa chão, bootstrap, novos
níveis ou executores; a contenção atual permanece. Referências abaixo ao
fechamento futuro não significam proteção já entregue em runtime.

O Hub controla o modo do Codex a cada início do motor e confirma o modo ao abrir
ou recarregar a conversa. O `config.toml` não sobrescreve esses dois controles;
as demais fontes de configuração ainda exigem o fechamento futuro (abaixo).
O Claude continua em `default`, com Pedir/Liberar decididos pelo Hub.

## Antes e depois

Antes, `CODEX_CONFIG` controlava instruções e memória, mas não sandbox/aprovação.
O `codex-acp` **2.2.2**, sem `INITIAL_AGENT_MODE`, escolhe **`agent`**:
`on-request`, `workspace-write`, rede desativada e `approvalsReviewer=auto_review`.
O adaptador reaplica esse preset em cada turno, acima dos valores de abertura
da thread. Portanto, fixar somente `CODEX_CONFIG` seria insuficiente.
Isso foi conferido no código da versão, sem iniciar motor ou consultar o Hub:
[AgentMode](https://github.com/agentclientprotocol/codex-acp/blob/v2.2.2/src/AgentMode.ts),
[abertura e recarga](https://github.com/agentclientprotocol/codex-acp/blob/v2.2.2/src/CodexAcpServer.ts),
[configuração e turnos](https://github.com/agentclientprotocol/codex-acp/blob/v2.2.2/src/CodexAcpClient.ts).

Agora, o Hub fixa `on-request` + `workspace-write` + revisor `user`, tanto na
configuração como no preset do adaptador (`INITIAL_AGENT_MODE=workspace-write`).
Confirma `configOptions` na abertura/recarga; modo diferente é corrigido e só
uma resposta que confirma o valor permite continuar. Sem catálogo no Codex,
ou se a confirmação falhar, a conversa não começa. Jobs internos conferem o modo somente leitura no Codex e recusam pedidos de ferramentas. No Claude, a sessão solicita bloqueio das ferramentas e dos MCPs persistentes; políticas gerenciadas remotas ainda não têm isolamento comprovado. Outros motores não iniciam esses jobs.

**Mudança observável:** Pedir pode mostrar escalonamentos que o revisor automático
do motor antes resolvia sozinho. Pedir e Liberar apresentam em cartão os pedidos enviados pelo motor, inclusive em configurações antigas. Isso não prova cobertura de efeitos que o motor não pergunta.
O Codex continua escrevendo na pasta e em temporários sem pedir. Rede do shell
e escrita em outras pastas passam pelo escalonamento normal; não há liberação
global de rede nem de pastas adicionais vindas do config do usuário. O host já
passava as pastas de memória em `additionalDirectories`, e esse fluxo permanece.
Rede do próprio motor para autenticar e conversar com o provedor não depende
da liberação de rede do shell. Necessidades específicas de bots existentes
precisam ser conferidas após o deploy; os perfis em execução não foram consultados.

Comandos customizados aceitam argumentos literais, seletores ACP e flags de
interpretador: `python3 -X utf8 -u script.py`, `python3 -m pacote`, `uvx pacote`,
`copilot --acp --stdio`, `grok agent stdio` e `~/bin/agente --acp`, por exemplo.
A lista de bloqueio continua recusando wrappers conhecidos de shell/ambiente
(`env`, `nice`, `sh`, inclusive em cadeia), atribuições de ambiente, avaliação de
código (`-c`, `--eval`, `--call`) e flags de configuração, perfil e bypass de
aprovação. `--acp=true` é um argumento literal, não uma atribuição de ambiente.
Só o runner conhecido `codex-acp` tem gramática fechada: direto sem argumentos,
ou `npx [-y|--yes] pacote-codex-acp[@versão]`, sem argumentos extras. Expansões
como glob, variável e substituição de comando não são aceitas; `~/` é permitido
somente no início do executável. Create/update validam antes de salvar e
mostram o erro em português; a inicialização também valida bots já salvos.

Essa checagem evita enganos e **não é uma barreira que inspeciona o programa**.
Node/Python podem iniciar um launcher genérico por arquivo ou módulo; um arquivo
escolhido pelo dono pode carregar `.env`, executar outro programa ou mudar seu
ambiente. Não se promete bloquear wrappers escondidos no conteúdo do adaptador.
O executável e seus arquivos continuam sendo de confiança; processos com o
mesmo usuário também podem alterá-los. O isolamento forte não está nestas
camadas.
O backend customizado também recebe a configuração protegida antes do handshake,
pois pode iniciar um adaptador Codex. Se outro motor se anunciar como Codex sem
essa configuração, o handshake é interrompido. Variantes pontuadas, perfis e
políticas do JSON recebido são retirados antes de aplicar os valores do Hub.
Os valores explícitos de rede e o array de raízes de escrita substituem os
inferiores; o adaptador acrescenta as pastas de memória. O `CODEX_HOME` do slot do motor
é descartado; o do processo do Hub, se configurado, é preservado para o login.

## MCPs do próprio Hub

Pedidos MCP enviados pelo motor chegam em cartão, inclusive os dos servidores internos.
As ferramentas do Hub continuam com suas verificações próprias; a presença do cartão
não prova que todos os efeitos de um motor sejam interceptados.
No Codex, o pedido para o servidor de memória também aguarda resposta: o adaptador
não comprova ao Hub a definição efetivamente carregada e pode retomar a sessão com
outra configuração. A guarda da memória é uma decisão separada e continua podendo
pedir confirmação após conteúdo externo.
Em pedidos do Codex correlacionados ao nome `memory`, o cartão e a ilha apresentam
**Quer usar uma ferramenta de memória**. A linha secundária **Nome técnico: …**
aparece somente quando o título, após retirar `Autorizar ` e `: memory`, começa com `mcp.`.
O cartão explica que o Codex não confirma ao app se esta é a memória deste bot.
Se a origem não for identificada, o cartão conserva o nome e a apresentação originais.
Nenhuma opção é escolhida automaticamente.

## Configuração persistente do Codex: limite atual

O modo fixo cobre sandbox e aprovação, mas não neutraliza `rules/*.rules`,
`mcp_servers`, hooks e `notify` do `CODEX_HOME` compartilhado. Um bot em Liberar
poderia gravar uma liberação que sobreviveria à troca para Pedir e afetaria outro
bot. Regras `decision="allow"` podem executar fora do sandbox sem cartão.
`prefix_rule(...)` sem `decision` também vale como **allow por padrão**,
inclusive quando escrita pelo próprio bot. Já `network_rule(...)` exige
`decision`: sem ele, o Codex recusa a regra com ``Missing parameter `decision` ``. MCPs e
hooks do usuário podem iniciar processos fora dele. O fechamento definitivo é
o **chão próprio**: editar a configuração do motor sempre pergunta.

O cartão do Codex filtra as opções que gravam regras permanentes no diretório
compartilhado: `accept_execpolicy_amendment`, `apply_network_policy_amendment:*`
(todas, inclusive o antigo "Never") e o `allow_always` de MCP (persistência `always`). Outras opções
`allow_always` desconhecidas e permissões de sessão também não são oferecidas.
O cartão só autoriza a chamada atual. O filtro também vale em Liberar. O sempre permitir
de verdade pertence às regras salvas no Hub previstas. O Hub guarda os IDs
filtrados de cada cartão pendente e recusa qualquer `optionId` fora deles antes de
responder ao motor ou à ferramenta. A recusa mantém o cartão pendente e retorna
erro em português ao cliente. Vale para app, voz e API; a voz também mantém as
restrições próprias. Reenviar a mesma opção de um cartão já respondido devolve
sucesso sem novo comando ao motor, inclusive após reiniciar o Hub. Outra opção,
cartão expirado ou cancelado continuam recusados. Cancelar continua possível.

Ao iniciar o Codex, a auditoria do diretório **apenas avisa** em português sobre
todo arquivo `rules/*.rules` com ao menos uma linha ativa: linha não vazia que,
depois de retirar os espaços das pontas, não começa com `#`. Essa checagem é por
linha e independe do reconhecimento de chamadas, aspas ou comentários no meio.
Arquivos vazios ou só com comentários não geram aviso. Marcadores reconhecidos
apenas detalham a mensagem: `allow`, `deny`, `forbidden` e `prompt` com aspas
simples ou duplas; `prefix_rule` sem `decision` permite por padrão, enquanto
`network_rule` sem `decision` é uma regra de rede incompleta. Sem marcador
reconhecido, o aviso diz que há regras que a auditoria não classificou e que
devem ser tratadas como allow, inclusive quando usam apelidos ou aspas triplas.
Também avisa de menção ativa a
`mcp_servers`, `hooks` ou `notify` no `config.toml`. O aviso aparece no chat e no
log do Hub com o arquivo relativo a `CODEX_HOME` e o tipo de marcador, sem conteúdo.
Os avisos vão sempre à conversa principal do bot, mesmo que o motor comece num
turno de canal. A publicação é deduplicada por bot no armazenamento do Hub,
inclusive após reiniciar: o hash considera o conjunto de mensagens e o SHA-256 do
conteúdo de cada arquivo sinalizado. Uma regra nova no mesmo arquivo volta a
avisar, mesmo que o texto público do aviso continue igual. Conteúdo e impressões
dos arquivos nunca aparecem no chat ou no log; o log mantém o aviso em todo início.
Na primeira inicialização após atualizar o Hub, se o hash salvo no formato antigo
(só mensagens) corresponder exatamente ao conjunto atual, ele migra para o novo
formato sem repetir avisos. Se o conjunto for diferente, publica normalmente.
A ampliação para todo arquivo com linha ativa ou a correção do texto de um
marcador pode mudar esse conjunto e republicar os avisos uma vez.
A escolha evita que uma autorização antiga do próprio dono impeça todos os bots
Codex de iniciar. Não se presume origem segura pela sintaxe do marcador: essas
configurações ainda podem dispensar cartões ou iniciar processos e precisam de
revisão fora do bot. Erro de leitura continua impedindo o início. É uma checagem
textual e conservadora; não interpreta toda a linguagem TOML/Starlark nem cobre
outras camadas, mudanças concorrentes ou executáveis modificados. Não substitui
o chão próprio nem a separação dos usuários do servidor.

## Credenciais

Todos os processos ACP, inclusive sondagens e jobs internos, recebem um ambiente
filtrado do Hub. Saem nomes contendo KEY, SECRET, TOKEN, PASSWORD, PASSWD,
CREDENTIAL ou AUTH, as famílias ANTHROPIC, OPENAI, XAI, OP, AWS,
AZURE e GOOGLE, além de URLs de banco e DSNs conhecidos. Caminhos de login,
PATH e locale comuns permanecem. O slot `agent-env` escolhido preserva a
configuração do próprio motor, inclusive modelos e headers de gateway, região
e token Bedrock e provedores do Codex. Famílias reconhecidas de outro motor
são removidas; o Codex aceita também os `env_key` declarados em `model_providers`
do `CODEX_CONFIG` (objeto ou chave pontuada). Nomes arbitrários do slot são
configuração explícita do dono, não credenciais herdadas do Hub. Overrides de
launcher/HOME continuam bloqueados. Outros motores recebem somente seu slot.
As duas entradas também bloqueiam inicialização automática (`BASH_ENV`, `ENV`,
`ZDOTDIR`, opções de runtime e de carregamento dinâmico) e proxies autenticados,
inclusive nomes em minúsculas. Proxies do Hub sem autenticação permanecem no
motor; credenciais embutidas na URL de um proxy não saem do Hub.
URLs com usuário/senha também são removidas de outras variáveis herdadas ou do
slot, como índices de pacotes; endpoints sem autenticação na URL permanecem.

Exceções operacionais explícitas são preservadas no ambiente ACP e na lista
final do shell Codex: `GIT_AUTHOR_*`, `GIT_COMMITTER_*`, `XAUTHORITY`,
`GOOGLE_CLOUD_PROJECT` e `LD_LIBRARY_PATH`. As demais variáveis de carregamento
dinâmico continuam bloqueadas. `SSH_AUTH_SOCK` é removida tanto do Hub quanto
do slot: o bot não recebe acesso às chaves do ssh-agent; Git por esse agente
deixa de funcionar. O login por assinatura não depende dele.

Checagens de login e o poller local de uso recebem a mesma fronteira do ACP,
incluindo apenas o slot do respectivo motor. Se esse slot estiver indisponível,
inclusive por cofre travado após o boot, a sondagem tenta somente o ambiente
herdado filtrado, para consultar o login por assinatura já salvo. Nunca recai
no ambiente completo do Hub nem destrava o cofre por conta dessa sondagem.
Se a autenticação antes vinha só
do ambiente do serviço, salve-a no slot correto pelo fluxo de autenticação ou
use o login por assinatura já salvo. Não copie chaves para documentação, shell
ou chat. O poller Claude usa `--setting-sources ''`, sem hooks/configurações
de usuário ou projeto.

Claude recebe `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`, mantendo a autenticação no
motor e removendo credenciais reconhecidas dos comandos, hooks e MCPs stdio.
Codex recebe `shell_environment_policy` com `ignore_default_excludes=false`,
`exclude` de credenciais e inicialização automática, `experimental_use_profile=false`
e um array final `include_only` de variáveis comuns (caminhos de login, PATH,
locale, temporários e runtimes). Esse array é aplicado **depois** de `set`.
O CLI mescla mapas recursivamente: `set={}` não apagaria um `set` inferior.
Assim, mesmo que um `set` do TOML inferior sobreviva, nomes de chaves, startup
de shell e proxies não pertencem à lista final e não chegam ao shell.
Uma lista literal de nomes fictícios testa essa fronteira, assumindo que todas
as entradas sobrevivem ao merge, sem dependência TOML adicional.
`allow_login_shell=false` evita que o shell reponha variáveis por perfil de login.
Os arrays `exclude`/`include_only` continuam oficialmente suportados. Se uma
configuração inferior misturar `filters` com esses arrays, o CLI recusa a
configuração antes do prompt: remova os filtros conflitantes, sem afrouxar a
política. Variáveis customizadas fora da lista final deixam de chegar ao shell,
assim como proxies, mesmo sem autenticação. Valores reintroduzidos por `set`
em nomes comuns permitidos não são um cofre: continua valendo a limitação de
processos com o mesmo usuário. A sintaxe foi conferida na
[documentação oficial do Codex](https://developers.openai.com/codex/config-advanced/#shell-environment-policy)
e na [documentação oficial do Claude](https://code.claude.com/docs/en/env-vars#what-the-subprocess-environment-scrub-removes).

Isso não isola arquivos de credenciais de processos com o mesmo usuário Linux,
não apaga logins nem substitui o proxy futuro. O scrub do
Claude depende da versão instalada e não remove todos os nomes possíveis;
em especial, a documentação atual preserva tokens GitHub e proxies. Por isso
essas credenciais do Hub já são retiradas na primeira fronteira. Não há garantia
de detectar um segredo em uma variável de nome arbitrário. Credenciais
configuradas em conectores MCP continuam pertencendo ao respectivo conector.

No Linux, o scrub do Claude também executa Bash em um namespace de PID isolado,
com `/proc` próprio: `ps`/`pgrep` não enxergam os processos do host e `kill` não
os sinaliza. Um processo em segundo plano iniciado no Bash não deve ser tratado
como serviço persistente nem presumido visível em outra chamada Bash. É preciso
conferir a versão instalada, bubblewrap e suporte/permissão de namespaces do
kernel; a falta desses pré-requisitos pode impedir o Bash ou a inicialização do
motor. O Mac não prova esse comportamento Linux. Não conclua que um serviço
do host está parado porque um `pgrep` do bot voltou vazio.

## Conferência antes de um deploy autorizado

No `CODEX_HOME` real usado pelo Hub, conferir **somente contagens**: existência
do `config.toml`, número de arquivos `rules/*.rules`, arquivos com ao menos uma
linha ativa (critério acima) e ocorrências dos tipos `mcp_servers`, `hooks` e `notify`.
Na contagem das regras, incluir todo arquivo ativo, mesmo sem marcador
reconhecido. `prefix_rule` sem `decision` permite por padrão (allow);
`network_rule` sem `decision` é incompleta e não equivale a allow.
Não imprimir valores, conteúdo de arquivos, comandos de regras nem `auth.json`.
Registrar o resultado antes do deploy; a ausência de marcadores não prova
isolamento nem substitui o fechamento futuro.

## Conferência manual depois de um deploy autorizado

1. Abrir uma sessão nova de um bot Codex de teste em **Pedir** e pedir um comando
   simples com rede, como `curl -I https://example.com`. Deve aparecer o cartão
   de escalonamento antes de executar com rede; testar Negar e depois Permitir. Use um bot de
   teste sem regras `allow` que já cubram o comando, para o cartão aparecer de fato.
2. Repetir em **Liberar**: o pedido deve aparecer em cartão. Reabrir uma
   conversa existente para conferir o modo também na recarga.
3. Conferir que o Codex edita um arquivo de teste na própria pasta e que a
   memória continua acessível. Se um bot precisa de outra pasta ou rede, o
   escalonamento segue o modo escolhido; não ampliar os roots globalmente.
4. Conferir pedidos de MCP interno e externo: se o motor pedir permissão, o cartão aparece.
   `manage` conserva suas verificações e `computers` exige aprovação local.

5. Conferir uma resposta curta no Codex e no Claude por assinatura, sem refazer
   login; repetir em um motor configurado com chave própria, se houver.
6. Para conferir o ambiente, usar só variáveis fictícias reconhecidas como
   credenciais e verificar **presença/ausência**, sem imprimir valores reais.
   Chaves do Hub devem estar ausentes no motor; chave própria do motor deve
   estar ausente no shell. Não ler `auth.json` nem arquivos de credenciais.

7. Num bot Claude de teste no **Linux**, executar uma única chamada Bash:
   `sleep 20 & p=$!; ps -o pid,comm -p "$$,$p"; kill -0 "$p" && kill "$p"; wait "$p"`.
   Registrar se o Bash inicia, enxerga e sinaliza seu próprio filho (o `wait` pode
   terminar por sinal). Em outra chamada, conferir `ps`/`pgrep` e o ciclo de vida
   de um processo em segundo plano de teste. Usar `kill -0` no PID conhecido de
   um processo descartável do host; não sinalizar Hub nem serviços reais. Conferir
   que o processo do host continua vivo pelo operador fora do bot.
8. Num `CODEX_HOME` descartável com fixtures de regra `allow`, MCP e hook,
   conferir o aviso no app com arquivo/tipo e sem conteúdo; o motor deve continuar.
   Em cartão de comando, rede e MCP, conferir ausência de opções persistentes e
   de sessão. Não inserir bypass no diretório de login
   usado pelos bots.

Nada deste checklist substitui a conferência no Hub real. A mudança vale quando
um processo novo do motor começar; processos já abertos precisam ser encerrados
e reabertos após o deploy pelo responsável.

## Validade do cartão

A resposta vale para o cartão pendente da conversa em que ele apareceu. Em canal,
o pedido informa a sala; o Hub entrega a decisão somente ao bot que pediu a
permissão. O app envia `botId`, `entryId`, `entryRev` (a revisão exibida),
`optionId` e, na voz, `source: "voice"`. Clientes sem conversa/revisão precisam
ser atualizados antes de responder. Isso não altera o formato das entradas.
Esses campos evitam respostas atrasadas ou enviadas à conversa errada, mas não
identificam quem responde nem são barreira contra quem tem o token local do Hub
(limite aceito até separar os usuários).

Antes de consumir a espera, o Hub confere bot, conversa, entrada, turno, sessão,
revisão e as opções filtradas do cartão. Pedido ACP de outra sessão é recusado
antes inclusive da aprovação automática. Cartão antigo, expirado ou cancelado
não libera uma ação. O reenvio da mesma opção já respondida só confirma o
resultado salvo, inclusive após reiniciar o Hub, sem nova entrega ao motor.

A voz mantém sua restrição: somente permitir uma vez em cartão classificado como não
sensível; os demais pedem “Aprove no app”, sem selecionar, negar, cancelar ou
consumir o cartão. Pelo app, negar ou cancelar continua possível, inclusive se o
membro tiver sido removido da sala; se o bot foi apagado ou seu ator não está
disponível, o Hub encerra o cartão salvo como cancelado, com nova revisão e sem
entregar autorização ao motor. Repetir a mesma resposta apenas confirma esse
encerramento. Cancelar não concede acesso, inclusive se o contexto do turno
tiver mudado. O cancelamento interno usado ao parar um turno também continua
funcionando.

## OpenCode: aviso no app

A OpenCode continua na lista de motores. Ao escolhê-la, o app mostra um aviso em
Novo bot e na seção Motor do editor do bot: *"Na OpenCode, a trava de segurança
não cobre tudo: ela pode usar configurações do computador dos bots e iniciar
extensões (MCP e plugins) sem pedir."* O aviso só informa: não bloqueia a
escolha, não troca o motor de nenhum bot e não muda o Hub.

A base é a prova registrada da CLI OpenCode V2: uma configuração global sobrepôs a do
projeto, e um MCP e um plugin globais rodaram antes do prompt, sem pedido ACP.
O que a OpenCode executa sem pedir não ganha cartão no Hub. O texto fica em
`apps/desktop/src/renderer/views/fork/aviso-motor/`; avisos de outros motores
entram no mesmo mapa, quando forem verdadeiros.

O motor oh-my-pi não está disponível para seleção nem para iniciar bots já configurados; escolha outro motor explicitamente.

O Bot de fundo usa uma conta escolhida nas funções gerais, independente do motor da conversa, e uma pasta temporária privada. Nesta entrega, a assinatura Codex não é elegível: uma sessão ACP extra não comprovou ausência de ferramentas e de configuração persistente. O login não é copiado nem vinculado. Contas e modelos sem isolamento comprovado ficam indisponíveis; a tarefa falha e o chat recebe aviso, sem troca automática de conta.

A conta global não recebe conversas de outros espaços: a tarefa é recusada antes da descoberta ou do envio enquanto não existir uma seleção autorizada para aquele espaço. No Automático, o login Claude local confirmado tem preferência; escolhas explícitas permanecem vinculadas. Se as vagas estiverem ocupadas, a tarefa aguarda.

Jobs Codex recusam `CODEX_CONFIG` externo não vazio e configuração global; no macOS o Hub consulta as duas preferências gerenciadas do Codex e recusa o job quando encontra uma delas ou não consegue consultar. Também recusa políticas locais gerenciadas do Claude, inclusive arquivos, fragmentos, MCP gerenciado e preferências MDM; falhas de consulta também impedem o início. O Hub usa somente propriedades próprias para configurar o job e avisa no chat quando ele não puder iniciar.

O dono aceitou que os jobs Claude continuem funcionando com um limite: regras gerenciadas remotamente podem executar ações antes da verificação do Hub. Em contas empresariais Team ou Enterprise, essas regras são definidas pelo administrador da organização; contas pessoais de assinatura não têm esse controle. Outras contas de organização, inclusive as que usam chave de API, também podem receber políticas próprias. Arquivos de política instalados no próprio computador são uma fonte separada.

No Linux, a presença ou impossibilidade de consultar `/etc/claude-code` impede a descoberta da conta e do catálogo antes de iniciar qualquer processo do Claude. A consulta de políticas locais ocorre antes de iniciar o job. Ela não impede que uma política seja alterada entre a consulta e o início; os testes simulam presença, ausência e falha, sem reproduzir o motor ou uma instalação MDM real. O login Codex por assinatura continua limitado e apenas o risco das políticas remotas do Claude é aceito.

## Credencial das pontes

As pontes MCP provisionadas em `session/new` e `session/load` usam uma credencial
Ponte própria, ligada ao bot, ao slot e ao vínculo de sessão originais. O Hub guarda
somente seu hash em memória e compara em tempo constante. A credencial é entregue
por `mcpServers.env` (`DONNABOT_PONTE_TOKEN`), sem persistência em settings, mirror,
arquivo ou log. O motor consegue vê-la: sua proteção é o escopo fechado, não sigilo
perante o motor. Reprovisionar o mesmo slot revoga a anterior; parar o bot, fechar
ou substituir a sessão invalida a credencial. Retomar a sessão reprovisiona a ponte.

A Ponte nunca é Local. Só admite as ferramentas existentes e os RPCs de transporte
necessários aos leitores MCP; pareamento, dispositivos, aprovações, regras, controle
do Cloud e instalação ficam recusados. Os contextos de conectores precisam pertencer
às definições entregues àquela sessão. As guardas de execução e de transporte
continuam valendo; a Ponte não cria Authority nem prova humana. A verificação de
MCP remoto pelo app permanece no Hub com Setup, sem sessão fictícia de bot.

**Limite atual:** com o mesmo UID, o motor ainda pode ler o arquivo do token Local.
A separação de usuário e a migração das raízes são necessárias para fechar esse acesso.
Esta etapa só retira Local das pontes e recusa a Ponte nos métodos do dono. Não entrega
isolamento de sistema nem o ingresso protegido. A linha de status do motor fica
pausada, sem credencial e sem fallback Local.

## Identidade Unix dos motores no Linux

Quando `/etc/donnabot/motor.conf` existe, o Hub lança processos Motor por
`/opt/donnabot/bin/donnabot-motor-exec`. O ajudante exige arquivo regular root
0644 (sem link), com `motor_user=<nome>`, `hub_uid=<UID>` e `work_gid=<GID>`;
recusa outro chamador, root e o próprio UID do Hub como alvo. O adendo atual também
recusa o GID primário do motor ou o `work_gid` iguais ao grupo primário do chamador
Hub. O instalador é responsável
por criar esse arquivo e instalar o ajudante root, executável apenas pelo grupo
do Hub, com `cap_setuid,cap_setgid+ep`.

O ajudante substitui grupos suplementares pelo grupo de trabalho, troca os três
GIDs e UIDs e os confere; zera capacidades permitted/effective/inheritable/ambient,
ativa `no_new_privs` e arma morte com o pai após a troca. Só então entra no cwd
pedido e executa um caminho absoluto, conservando pipes, ambiente explícito e
grupo de processos. Login PTY, inclusive `_meta.terminal-auth`, sondas, uso,
consulta local de Lugares e comando de terceiro na verificação MCP usam essa
fronteira. A ponte HTTP de setup MCP continua no Hub com credencial Setup.

Instalação de adaptadores/Node, npm e extração em raízes do Hub são recusados
nesse modo até a migração das raízes; nunca executam código de terceiros como
Hub. O kill do grupo e a limpeza geral são subcomandos sem shell do ajudante.
EPERM na consulta do grupo significa vivo. A unidade permanece de usuário e,
nesse modo, inclui `UMask=0077`, limpeza em `ExecStartPre` e `ExecStopPost` com
conferência de ausência de processos executáveis do motor. Não usa
`NoNewPrivileges=` na unidade, pois bloquearia as capacidades do ajudante.

Sem configuração, os lançamentos e a unidade conservam o comportamento anterior.
Esta etapa não instala nem ativa o modo em produção. O ponto de encaixe para
Landlock fica entre cwd e exec; liberações e isolamento entre bots pertencem a
etapas futuras. Raízes, perfis, instalação e aceite do serviço real pertencem
às etapas seguintes. Nenhuma ferramenta, autoridade ou capacidade nova é anunciada ao bot; por isso
esta etapa não altera o manual do harness.

A prova deste modo encontrou `PDEATHSIG=9` no exec, mas o filho de outro UID sobreviveu
à morte do pai na VM. A limpeza pós-saída é a garantia necessária inclusive
para o filho direto, além dos netos; não depender de PDEATHSIG sozinho.

O encerramento acompanha cada operação: filhos Motor têm grupo próprio e uma
guarda que chama o ajudante no kill explícito, timeout ou cancelamento. Nesses
filhos, `kill_on_drop` do Hub fica desligado; o Hub não sinaliza outro UID.
Verificação MCP e sondas encerram o grupo antes de esperar, com espera limitada
após o kill. Fechar o PTY legado/terminal-auth ou abandonar o login PTY de setup
encerra seu grupo pelo ajudante, mantendo o Hub ativo. A verificação HTTP com
Setup continua sendo processo Hub e conserva seu encerramento local.

**Limite atual:** um descendente que chama `setsid` ou `setpgid` pode escapar
do kill-group por operação até a limpeza geral (`cleanup`). Nesse intervalo, o
UID único mantém os direitos do motor; o grupo por operação não entrega contenção
desses descendentes. O tratamento e a prova desse limite ficam para etapas futuras.

## Raízes e leitura delegada

No modo separado, `data_dir` é privado do Hub (0700); área de trabalho, anexos,
probe e cópias conferidas de skills/manual ficam fora dele. As bases de trabalho
usam o grupo de trabalho e 2770; anexos publicados usam pastas 2750 e arquivos
0640, aplicados no descritor seguro. Perfis de conta e pastas efêmeras de job
são privados do motor (0700); o ajudante os cria, valida, protege e remove.
O Hub conserva configuração, histórico e dados privados no `data_dir`.

`send_file` lê com o UID do motor pelo ajudante, até 100 MiB e sem seguir links
na folha nem em ancestrais. `manage` só aceita a raiz de trabalho nesse modo.
Memória/wiki ficam no Hub e são lidas pelas ferramentas lógicas. A mudança não
amplia autoridade nem instala o isolamento entre bots. Configuração ausente mantém os caminhos
anteriores. A especificação das raízes e da migração
registra a interface que o instalador chama com o serviço parado, backup e lock exclusivo;
a instalação e a comprovação de migração e uso real pertencem às etapas seguintes.

No modo com UID separado, a auditoria do CODEX_HOME selecionado (inclusive no
background) roda pelo ajudante como motor. O Hub recebe somente avisos e hashes,
sem abrir os perfis 0700. Contas CLI legadas migradas são sondadas no perfil
accounts/<id>/legacy e revalidam a identidade; perfis com origem compartilhada
são reescritos por conta, preservando destinos distintos e a migração idempotente.

## Fronteira dos instaladores

O instalador é um subprocesso do próprio host, iniciado pelo ajudante como motor —
disponível após a migração das raízes; antes dela, a instalação segue recusada
como acima.
Download, integridade, extração, npm e promoção não executam no Hub. A consulta
de descoberta/estado também ocorre como motor. O Hub confere a publicação na
raiz do motor por uma operação com descritores sem links e mantém recusados os
executáveis privados antigos do Hub, inclusive em perfis já migrados.

A fronteira de segurança desta etapa é o UID e as permissões dos dados privados
do Hub. Não há Landlock no instalador: um script pode escrever em `/tmp` com os
direitos do motor. Conter essas escritas é endurecimento futuro, junto do Chão;
não é condição para esta entrega nem autorização para liberar pastas por bot.

### Caminhos privados antes da troca de default

As famílias `~/.mexerica*`, `~/.donnabot*` e `~/.codync*`, incluindo Dev,
sandboxes e backups, continuam privadas. A validação também recusa seus aliases
com destino fora da home. A pasta configurada por `MEXERICA_HOME` continua
protegida, inclusive fora da home; as exceções gerenciadas existentes do próprio
bot/canal permanecem. Esta etapa preservou o default anterior; a troca seguinte ativa o novo
default e `MEXERICA_HOME`, sem movimentar os dados.

Não tente contornar recusas por shell, links, outro bot ou outro usuário. No
Linux com motor separado, o ajudante executa com o UID do motor; leitura, escrita, cwd e
`send_file` dependem das permissões dessa identidade. Pastas privadas e backups
com segredos precisam conservar dono do Hub, 0700/0600 e nenhuma ACL que conceda
acesso ao motor. A orientação do harness e a validação de pasta não substituem
essas permissões e não isolam processos que compartilham o UID do Hub.

O backup ignora árvores privadas aninhadas no workspace pelos três prefixos,
preservando as exclusões antigas; o backup próprio do Hub continua selecionando
sua raiz configurada. Este corte não move dados nem instala/ativa serviços. O
inventário correspondente registra limites e provas pendentes.


Com a troca: a seleção própria agora é `MEXERICA_HOME` / `~/.mexerica` (Dev:
`~/.mexerica-dev`), sem fallback legado. As negações anteriores continuam e reconhecem
diferenças de caixa; recibos regulares continuam preservados pelo backup.
Consulte o [contrato e limite do desktop](../guides/mexerica-recursos.md).
Nenhuma instalação, mudança de UID/GID ou movimentação de dados acompanha o patch.

### Chave OpenCode Go

A chave informada na folha do OpenCode usa o slot cifrado do cofre do Hub e é entregue pela variável `OPENCODE_API_KEY` ao processo do motor. Não é incluída em comando, URL ou logs da Mexerica. O CLI continua responsável por proteger seus próprios dados. A presença da chave não comprova autenticação nem autorização de uso da assinatura.

### Terminal de login remoto

O passo `login` de `agentSetup` reutiliza o PTY de `host/src/agent/term.rs`, com
política distinta dos terminais de instalação. Só resolve binário conhecido e
argumentos estáticos do Hub; não passa pela shell nem aceita comando, ambiente,
pasta ou argumentos do cliente. A lista é fechada em Cursor e Grok;
todos os demais motores permanecem bloqueados sem prova própria contra escape. A origem original `engine.accounts`/ACS4 é
capturada antes de filas e acompanha a sessão; somente o mesmo titular pode ler,
digitar, redimensionar ou cancelar. As guardas são reconferidas na entrega de cada
saída e na escrita efetiva no PTY, além da entrada autenticada da API.

O perfil da conta padrão de cada motor não gerenciado é separado por Space em
pasta 0700, sem links, sob o usuário do motor. HOME/XDG próprios e ambiente mínimo
separam o login de projetos e do ambiente do Hub. Em motor separado, a criação e
validação passam pelo helper de perfis existente, com ID derivado e geração
aleatória na raiz restrita. Cada tentativa tem pasta provisória própria: só a
conferência positiva seguida de `commit_and_admit` sob o vínculo vigente publica
o perfil para o runtime. Cancelar, revogar ou expirar encerra o grupo e apaga a
provisória, preservando a pasta e a conta anteriores.

O login não mantém scrollback nem permite replay ou um segundo leitor. A leitura
nativa só começa depois de o titular assinar o stream; queda do canal cancela a
operação. Os bytes vão somente ao canal privado do terminal, nunca ao evento geral,
chat, mirror, logs ou analytics. O histórico de instalação permanece com seu
comportamento anterior. OSC/clipboard são removidos. Setas/Enter são fechados;
Ctrl-C/Z/D, escapes arbitrários, comandos e multiline são recusados. Código só é
aceito após URL HTTPS de host oficial, fica em memória até Enter e tem envio
único. Após enviá-lo, o Hub descarta toda saída nativa, inclusive eco do código.

Confirmação automática usa consultas sem prompt/cota, com espera inicial de três segundos e recuo até 60 segundos. Após o envio, consulta imediatamente e a cada segundo, limitada a cinco segundos. Ausência de sonda, timeout ou resposta desconhecida
nunca viram confirmação. A sessão fecha ao confirmar, sair do CLI, cancelar,
revogar ou alcançar nove minutos. A revogação também é conferida durante a sonda.
O canal de reenvio é preparado fora da cerca. `Authority::commit_and_admit`
mantém as mesmas guardas originais de transporte e contas desde a transação dos
cartões e mensagens até o enqueue síncrono. Uma revogação antes dessa admissão
não deixa mensagens `queued` para recuperação posterior do ator. Uma origem em
task-local, sozinha, não autoriza esses efeitos.

O método remoto `engineGoKeySave` aceita apenas `OPENCODE_API_KEY` no OpenCode, sem `spaceId`, `engineAccountId` ou outras variáveis. `setAgentEnv` permanece local.
O cofre cifra fora da transação e grava sob a autoridade original de contas,
reconferida no commit. A resposta contém nomes, nunca valores. Os outros caminhos
de escrita do cofre continuam locais. Guardar a chave não confirma assinatura.

O chat pessoal do Antigravity usa apenas o CLI oficial verificado com `agy acp`.
Cada prompt requer uma mensagem digitada no chat, de um aparelho autenticado do
dono, vinculada ao turno. Um pedido HTTP local não fornece essa autorização.
Rotinas, delegações, canais externos e reenvios automáticos não usam o login
pessoal. O Hub não copia credenciais, não usa o catálogo como alternativa e
não inicia o login. Pedidos ACP de aprovação só recebem uma decisão pelo cartão,
sem aprovação permanente; as regras nativas do CLI podem permitir ações que
ele não encaminha ao cliente ACP.
