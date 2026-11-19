# Mexerica — consumidor pessoal PL1 no Hub

Estado atual: recorte vertical **Host C3b.2 candidato**, congelado antes de
compilar. Não conclui Main/PIN/runtime, C3b.2 inteiro ou C4. C3b.1 tem SIM25
e o merge tem SIM26; a base3d34 teve fmt0 e personal61/61 pelo root (inclusive
a MESMA regressão R futuro antes vermelha). Não é Clippy/workspace verde.
Os registros de R1/R2/R3/24 e ausência de prova abaixo são históricos.
Fonte normativa:
contrato §11
e registro C3a.
Review19 C3a foi NÃO ESTÁTICO por recursive_triggers e vínculo DB/preflight.
Correção R1 candidata para nova revisão20, sem prova de compilação/execução.
Full fmt/Clippy/workspace/CI e revisão final continuam pendentes.

`manifest.rs` é a única fonte admitida: ADMITTED=None. `protocol.rs` confere
bytes JSON canônicos e deriva o hash de C sem interpretar um grant ACS4.
Inspeção sintética e injeções de FS/clock são privadas de cfg(test); arquivo
root, body, env, health, banco ou renderer não fornecem raiz.

`donnaPersonalBootstrapStatus` está no despacho real e em LOCAL_ONLY. Exige
Caller::Local e body `{}`. Resposta: `{profile:"personal-linux-v1",wire:1,
available:false,reason:"updateRequired"}` com o artefato atual. Mesmo uma
admissão futura de manifesto retorna `helperRequired` neste recorte. A chamada
não publica configuração nem muda Store, posse, prazos, canais ou identidade.
Não é ferramenta do bot nem inspeção independente de Q.

C2: `shapes.rs`/`wire.rs` verificam os 30 tipos fechados, assinaturas, C/T,
antecedentes, caps e prazos; POP abre B com H.box, X25519 que recusa DH zero,
HKDF/AAD e ChaCha20-Poly1305. Signed conserva bytes privados e expõe getters.
Reader App/Hub e Time Current/Committed/Historical são decisões internas,
sem parâmetro de renderer/IPC. Historical só inspeciona originais vencidos;
não pode autorizar commit/canal. `verify_open` exige metadados HTTP exatos.
C3a acrescenta três verificadores restritos de time-request H, time E e
snapshot E: não exigem A/tupla nem epoch fictício. A fonte continua o C/E/H
admitido do consumidor; snapshot sozinho só dá um limite de tempo vinculado,
não promove nem concede autoridade. As igualdades Y/W de C2-1 permanecem.

C3a liga `provisioning::prepare` no serve **antes** de open_store e de
Identity::load_or_create. Lê config fixa `/etc/donnabot/bootstrap-personal-v1.json`,
journal `/etc/donnabot/bootstrap-personal-v1.journal` e evidência read-only do
DB. Não corrige, apaga ou reescreve estado divergente. `secure_fs.rs` abre cada
pai por descritor/no-follow; root usa UID0/0750/0640, identidade usa eUID real
do serviço e data-dir0700/identity0600; nlink1 só para arquivos regulares.
O loader usa a H existente com ambas as chaves conferidas, antes de hub.start,
background e ingressos. `runtime.rs` retém somente fundação/clock, sem estado
ativo ou promoção. Info/subcomandos não instalam geração pessoal.

O formato fechado do journal consumidor e sua evidência de seed estão em
§8.
Root não basta para epoch: pedido H e resposta E precisam ligar nonce, C,
Hub/boot/RTT à seed exata. Sem journal, Clock fica sem epoch até nova E.
`clock.rs` usa CLOCK_BOOTTIME+bootId Linux, sem parede/Instant alternativo;
RTT <=5s, seed <=300s, leases <=900s. E em memória pode renovar referência
expirada antes de A/T; não estende T/G. Restore do lease revalida bytes E e
bSend/receive/deadline originais, não recebe prazo novo no restart.

`store/personal_bootstrap.rs` e SQL local compartilham Store.db e transação
com A1/ACS4. `first_owner` e origens pessoais são imutáveis; triggers excluem
perfis nos dois sentidos, inclusive REPLACE de marcadores. Backfill só usa
fatos existentes. Birth local só para DB criada exclusivamente por este
Store; DB antiga vazia/perdida não comprova instalação virgem. Instalação
pessoal cerca inclusive Device Local pareado depois, sem V/lease atuais ou
após remover Device. É origem para C4, não uma Authority de cofre.

A1 sem config assina sob a mesma Connection/transação depois de reservar
origem. Conserva primeira evidência e ever_claimed; novos inputs Local
legítimos continuam aceitos após cancelar/expirar intenção anterior. ACS4
configurado conserva a idempotência original de mesmo input; PL1 impede
qualquer callback de assinatura A1. O callback não faz Store/Cloud/I/O/await.

D62: processo com acesso à H no mesmo UID do Hub ainda pode assinar como H;
não há isolamento novo nem journal root que elimine isso. Helper/produtor,
instalador, usuário/unit e proteção real do sistema pertencem ao PR4 e não
estão prontos. Rede/journal mutante/posse/promoção/canais são C3b; Authority,
paridade de contas/chaves e UI real, C4, no mesmo PR3. SSH/Caller::Local e ACS4
atuais ficam preservados. Não há nova oferta, raiz real, serviço ou cofre
remoto por ter esses módulos. Só Operação instala/copia/abre app no Mac.

C3a R1 propõe `store/personal_database.rs`: toda Connection de Store liga e
confere recursive_triggers antes de migrações/escritas. Isso faz o DELETE
implícito de REPLACE respeitar a retenção de geração ACS4 reivindicada, sem
depender do default SQLite. Os guards/SQL, ever_claimed e retry A1 sem config
não foram relaxados.

No startup pessoal, Prepared conserva a Connection existente que leu o DB,
a âncora File e a evidência da geração/first_owner/proveniência, incluindo
claims/tombstones/revisão. O handle é aberto RW **sem CREATE**, mas a preflight
só faz leituras/transação de leitura, com query_only=ON conferido na conexão;
não migra/instala. A escrita só é liberada após validar o handoff.
Após qualificação, Store::open_personal consome essa MESMA Connection. Antes
de inicializar Store, confere identidade do arquivo e SQLITE_FCNTL_HAS_MOVED
no VFS real, relê e compara a evidência. Arquivo removido/renomeado/trocado ou
história divergente recusam sem criar DB/birth/H ou reparar. Não reabre por
pathname para tornar a Connection gravável nem aceita descritor de cliente.
VFS incapaz de confirmar o handle recusa. A fábrica genérica/não Linux conserva Store::open; o ingresso canônico Linux usa open_personal/open_created, sem reabertura após qualificação.
Esses testes candidatos usam APIs reais de Prepared/Store e não foram rodados.

C3a R2 propõe fechar o ingresso CLI residual do NÃO estático20. O open_store
canônico compartilhado por install/reset-token e fallback legado do serve
consulta estado pessoal ANTES de criar diretório/DB/birth/host_id/token/H.
No Linux, presença de configuração ou journal pessoais (mesmo inválidos)
recusa; origem pessoal durável na DB também recusa sem migrar. Não chama
prepare para instalar geração em subcomando. O fluxo próprio desses comandos
pessoais ainda não está disponível; isso não é recuperação entregue.
Sem perfil pessoal, DB existente usa a mesma Connection capturada/verificada;
DB ausente pode ser criada pelo fluxo legado normal. Store temporária de
outros módulos/testes não ganha leitura root global. No serve, prepare passa
antes do lock que poderia criar a pasta; ramo pessoal conserva o handoff B2.
B1/SQL/clock/wire e prazos não mudam. Quatro testes novos candidatos exercitam
a mesma fábrica/guarda do CLI com FS/SQLite reais e leitura root privada de
fixture; não instalam serviço nem executam produto. Ainda sem compilação/
testes C3a, aguardando nova revisão21 independente e gates finais.

C3a R3 candidato fecha a janela de criação apontada pela revisão21 (NÃO
ESTÁTICO). No ramo canônico Linux após capture(None), não há fallback à
abertura genérica: create_new deve reservar o nome exclusivamente; AlreadyExists
recusa. CreatedDatabase conserva FD e identidade do arquivo criado, abre
SQLite RW SEM CREATE/query_only, verifica mesmo arquivo/path/HAS_MOVED e
estado ainda vazio antes de entregar a MESMA Connection ao Store. Só essa
capacidade privada permite new_database/birth nesse ramo. Substituição antes
da Connection ou do handoff recusa sem migração ou remoção de arquivos.
Guarda root e handoff existente não mudam; Store genérica de testes/outros
perfis não recebe root global. Foram escritos quatro casos novos (restauração
após ausência, duas trocas de arquivo e controle ACS4) e reforçado o controle
de criação legítima. Nada executado; nova22 e gates finais ainda pendentes.


### C3b.1 em desenvolvimento, sem gate ou aceite

C3a recebeu SIM **estático** NOVA22 para tree45bf; nenhum Rust/fmt C3a foi
executado. O candidato C3b.1 acrescenta consumidor FD3 e sessão helper↔Hub,
HTTP H enumerado, releitura root por revisão, slots e disputa K/F/R×W no mesmo
SQLite FULL. O listener é entregue root bind-sem-listen; o serviço dedicado
valida sockfs e pathname separadamente, marca CLOEXEC antes de spawn e chama
listen com seu UID. PR3 não cria socket, launcher, unit, writer ou helper root.
Ausência/incompatibilidade recusa; legado não toma fd3. PR4 deve provar peers
reais e o produtor. Uma sessão conserva captura/T e admite um pedido em voo;
reconexão sem esse vínculo não enumera preparos pelo UID0.

O laço /personal-possession é E2EE com transcript próprio vinculado a C/T/G/A/H,
só aceita challenge(G), proof(G,K,F), R/close e nunca chama dispatch. K é único;
o PRIMEIRO K exige o testemunho de consumo assinado pela raiz E (read do Host, `consumedAt` da linha
D1), um único read de até 5 s, e só congela com lower >= max(G.issuedAt, consumedAt); retry devolve os
mesmos bytes (HOST-R1-01). Fundação desligada: PIN false/DON-862.
R, pending, originais, revisão/ACL, grant e ever_claimed compartilham commit.
Não cria Device ativo. W nulo grava Y nulo e tombstone, sem liberar geração.
O supervisor pessoal não entra nos hooks/mailbox/ACS4 legados. Publicação,
E/V/uso/pin/ready e fechamento completo permanecem C3b.2; paridade/UI é C4.

O contrato da ponte de preparo recebeu SIM contratual23. O candidato agora
liga /personal-prepare E2EE (candidate command/enrollment device), com captura
Q/H original e autenticação A command/NULL device; nunca Caller/Root/RPC.
Os formatos e a custódia estão em bootstrap §11.3.
O helper mantém seis ops internas e cinco externas, sem candidate extra.

Candidate no Hub retém declaração pública/digest/CAS FULL e possível envio,
sem raw token ou corpo em personal_sessions. Unknown só lê; não reenvia POST.
Begin original é cifrado com H.box_secret/XChaCha em domínio próprio, fora das
travas; fatos/cifra entram no mesmo commit. DeviceCode fica exclusivamente no
Hub e é retido até terminal autenticado apropriado, inclusive depois G/R e
publicação. PR3 não cria keyring, Rootwriter ou nova H. D62 continua: mesmo UID
com acesso a H não é isolado por rootjournal. Releitura verificada por revisão
precede CH/L; promoção/uso exigirão mais que relógio ou journal em memória.

Testes candidatos incluem composição Main/Rust dos mesmos bytes, handler de
preparo/posse sobre WebSocket isolado, SQLite/ACL/FS/crypto reais até R,
F×W/A1/ACS4 e retenção/unknown. Transporte Worker e ator root de fixture são
controlados e privados cfg(test); não provam Worker runtime, produtor PR4 ou
peers privilegiados positivos de instalação. Há teste de SO_PEERCRED real e
FD recusado sem fechar descriptor. Nenhum teste/fmt/Rust C3a/C3b foi executado
nesta rodada. A revisão22 foi estática da fundação; SIM23 foi do contrato.

R não ativa Device nem publica. Ingresso/dispatch genérico de Devices de
origem pessoal durável recusa antes de unlock até admissão explícita C3b.2/C4,
inclusive Local pareado depois. SSH Caller::Local e ACS4/iOS legados seguem.
C3b.1 completo candidato será congelado para nova revisão24, sem autoaceite;
ADMITTED=None, Main sem M3 real, Worker null e SSH preservados.

### Patch mínimo R24 — candidato, sem prova pós-correção

Depois do POST/device e da cifra fora da trava, o callback final da MESMA
Connection exige s.preparing(), configuração/Q/_begin e _sentBegin exatos.
Claim Local/fechamento durante a espera recusa fatos+cifra; preserva originais
e _sentBegin unknown, POST único e geração/ever_claimed/proveniência. Não apaga
código previamente retido por mero prazo/G/R/publicação, nem cria recuperação.
Main também requalifica a captura original após journal/token/handshake e antes
de helper/status/F/HTTP com efeito. Os testes vermelhos e fixtures foram
preservados; provas pós-fix e revisão nova25 ainda pendentes. Nada disso entrega
publicação/ready/C4 ou produtor PR4; ADMITTED=None/SSH e demais gates mantidos.


### Guarda causal de R — patch mínimo candidato

O instante H qualificado usado em R/pending é capturado e comparado com F[7]
antes de claim/remember/ACL/assinatura/freeze de R. Se H ainda não alcançou F,
recusa sem esperar nas travas e a mesma tx FULL desfaz efeitos/originais novos.
Validade é rechecada após ACL e antes do commit; não restampa G/K/F/R nem
estende prazo. R antigo inválido não é reparado por overwrite/novo recibo e
não autoriza posse. A regressão real foi vermelha no servidor na6ab2; novo
resultado pós-fix ainda pendente. Lower/upper H completo, sync/relay e C4 não
foram implementados por este delta; nenhuma capacidade foi habilitada.


### Host C3b.2 — candidato para focal, sem execução deste delta

`epoch.rs` separa lower de emissão e upper de validade a partir da time E ou
clock_evidence já autenticada pelo leitor root. `clock.rs` conserva a mesma
Reference/deadline/boot: os limites de ms e RTT não reancoram prazo. BOOTTIME
é preparado antes das guardas; amostragem posterior não lê FS. As esperas
causais antes de CH/K/F/V ocorrem fora de Cloud/runtime/DB; a transação pode
recusar/rollback sem congelar original futuro. P/L/U recusam até lower alcançar
o predecessor, sem assinar/fixar valor artificial.

`live.rs` guarda E/request/lease/V/Device/revisões na mesma SQLite FULL; promoção
usa draft SELECT, validação/assinatura fora das guardas e CAS pós-triggers na
ordem Cloud → runtime → Connection. Owner vem apenas do E positivo da mesma
T/G/R/publicação. Primeira promoção não exige V nem relay anteriores. Restore
reverifica originais e deadline; não aceita automaticamente V nem conecta.

`sync.rs` envia somente R original, confirma publicação por Read, pede E com
SnapshotFlight, promove e registra possibleSend antes de activation. Unknown
não cria outra V para destravar. Reconciliação de receipt pode transmitir
somente o MESMO corpo durável idempotente admitido pelo Worker, após Read atual;
a ausência não prova não envio e nunca autoriza novo G/K/F/R. Os demais efeitos
não herdam essa regra. Read de device usa o código cifrado mesmo após R/prazo.

`relay.rs` é o supervisor próprio, sem register/pull/hooks/mailbox/ACS4. WS GET
vazio nos paths C2 usa assinatura HTTP e Open13 próprias com mesmo kid/nonce/
path; activationCurrent da V exata precede abertura. Somente socket real aberto,
V aceita, snapshot e lease atuais permitem cloud_synced. Isso NÃO é ready do app.
`probe.rs` recebe admissão privada desse socket/V/open e não cria Caller. Só
handshake, hello público restrito e donnaPersonalSessionProof nonce32 (uma vez,
5s, <=80KiB). Guarda após espera/fila e no escritor; nenhum dispatch genérico,
terminal, attachment ou segredo. O hello normal/mirror10 não muda.

`fences.rs` conserva W/Y/X por revisão append-only e latestCAS. W nulo após R
bloqueia ingresso, mantém R e Y nulo; W superior com R remove só o Device exato
e grava X/Y/revisões pós-triggers no mesmo FULL. ACK antigo/NoY não limpam segredo
nem anunciam revoked. Só S terminal autenticado que confirma W/Y/X atuais pode
limpar o ciphertext deviceCode; fatos/reservas/origem/ever_claimed ficam.

Os oito testes novos Host cobrem emissão/quantização H, espera antes de R,
promoção/ACL/CAS/reopen, E2EE do ChannelKind pessoal W nulo→superior com R e o construtor HTTP/Open13 real.
Worker/root/relay são fronteiras externas controladas privadas; SQLite,
assinaturas, KDF/canal e FS são reais. Não provam upgrade Worker real, produtor
PR4, Electron/pacote instalado ou fluxo Main→PIN. Nenhum teste executado pelo
dev. Main permanece byte a byte na3d34; seus clock/runtime/P30/cold-S e o
composto Main/Host ainda são parte obrigatória do mesmo PR3, antes de C4/ready.

### Encaixe ACL após main4049 (#202) — correção candidata

Na integração972, a época privada OAuth criada após INSERT causava um UPDATE
interno que o trigger pessoal contava novamente. O upgrade de
`personal_acl_update`, na mesma transação de migração pessoal, observa as nove
colunas Device anteriores por `AFTER UPDATE OF`: inclusive NOOP/name continuam
invalidando revisão/CAS. Somente a escrita isolada de `donna_oauth_epoch` não
representa outra mudança da ACL pessoal. Triggers OAuth/ACS4/INSERT/DELETE,
recursive_triggers ON e igualdade pós-trigger de V permanecem intactos.
Sete focais ficaram vermelhos na972; provas do fix ainda pendentes. MainM1
está pausado, seus runtime/clock/P30 e C4 seguem obrigatórios no mesmo PR3;
mirror efetivo12 e SSH/None/null preservados, sem ready genérico.

# M2 — lifetime fechado do probe em desenvolvimento

Depois das MESMAS duas RPCs, o handler conserva a admissão/permit e o deadline
original de5s enquanto aguarda peerclose. Qualquer frame adicional recusa e
fecha, sem dispatcher, novo Owner/Use, renewal, touchDevice ou analytics.
A saída da fila e a cifra são seguidas de recheck antes de enviar. Fechamento
ou expiração não prometem ausência de cerca ou aceite do PIN/Main.
Focais novos usam handler/Hub/SQLite/crypto reais e MPSC/tempo externo
controlados; ainda sem execução por41. Conferência do executor+gates próprios
substituem revisão independente conforme ordem do dono de04/11;43 permanece
interrompida. M2 vertical/PINFS/C4 não estão entregues por esta nota.

