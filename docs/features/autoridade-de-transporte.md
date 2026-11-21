# Autoridade original de transporte da Mexerica

O Hub conserva a autorização que recebeu cada solicitação. Revogar e autorizar novamente o mesmo aparelho não torna válida uma solicitação antiga. Troca de dono, escopos, origem ou concessão exige uma nova solicitação. Se o acesso vencer ou a sincronização for perdida enquanto o pedido aguarda, o app precisa confirmar o acesso atual e fazer uma nova solicitação. A volta da conexão não recupera pedidos Account anteriores à perda de sincronização.

Esta autoridade identifica o transporte e sua capacidade de controle. Ela não aprova efeitos, não prova presença humana e não é `HumanReviewProof`. A prova humana comum tem produtor único próprio, fora deste núcleo. Control e `owner.devices` não concedem `engine.accounts`, nem transformam Device em Local. Não há ferramenta de bot, RPC ou serviço novo.

## Contrato do produtor

O produtor único é `fork::autoridade::Autoridade`, extraído das finalidades de `AutoridadeEditor` do código próprio Chão2B. Não depende de chão, níveis, preflight, política, inicialização, selo, runtime ou editor. Skills e Chão2B devem consumir esse produtor depois do aceite, sem copiar um resolvedor nem criar outro DTO de autoridade.

| API interna | Quem usa e garantia |
| --- | --- |
| `autenticar(&Hub, chave) -> (Device, Caller)` | Handshake do canal e mailbox. Lê Device estrito e geração persistente na mesma Connection/TxImmediate, sob Cloud → DB → cerca de sincronização, capturando também a época de perda nessa mesma cerca. O handshake só admite o Caller depois de conferir a assinatura; mailbox só despacha depois da abertura criptográfica. Não aceita identidade fornecida em argumentos de ação, tag, modelo ou flag. |
| `reconferir(&Hub, &Caller) -> (Device, Caller)` | Rechecagem do canal. Conserva chave/origem/grant/escopos/dono/gerações; somente uma nova requisição pode receber o lease e a época de sincronização correntes da mesma concessão. Recusa ABA entre handshake e rechecagem. O Arc de pedidos já entregues à fila continua intacto. |
| `Autoridade::capturar(&Hub, &Caller)` | Consumidor de controle, antes de fila/await. Recupera o retrato privado original; nunca adota uma concessão corrente pela chave. Exige Control original. Local vem do adaptador loopback com bearer autenticado; `DeviceSource::Local` continua Device. |
| `conferir(&Connection, dono, época_dono, época_sync, sincronizado)` | Guarda interna na Connection protegida. Compara chave, origem, grant, escopos e geração; Account exige mesmo dono/época, sincronização atual na mesma época de perda capturada, prazo original vivo e lease corrente não reduzido. Não chama Store/Cloud. |
| `with_db(&Hub, Option<&AuthoritySAAS04>, callback)` | TxImmediate própria: confere antes do callback e antes do commit na MESMA Connection. Callback síncrono só Connection, sem Store/Cloud reentrantes, await, rede, espera ou saída irreversível. |
| `com_saida(&Hub, Option<&AuthoritySAAS04>, enqueue)` | Resposta pronta e vaga reservada antes. Reserva de leitura TxImmediate + cerca única; toda validação precede o último enqueue síncrono. Não há escrita, commit ou pós-validação falível depois de enviar; o Drop libera a reserva de leitura por rollback. |

As falhas de leitura de origem/escopos/geração são recusas, sem converter corrupção em identidade válida. O Caller possui retrato RAM não serializável, sem construtor público de concessão. Os Callers legados internos usados para verificar permissões podem não carregar retrato; eles não podem produzir `Autoridade` nem saída protegida. Os transportes reais sempre carregam o retrato. O dispatch revalida os retratos que recebe antes do roteamento.

`Hub.auth` é o watch de invalidação: desperta rechecagens e publicação da ACL; não é credencial nem geração persistente. A correção não depende de a notificação chegar antes de um pedido: DB e retrato original recusam o acesso por si. O adaptador HTTP conserva o caminho loopback + bearer, sem aceitar Caller do JSON.

## Concessão persistente e prazo

`donna_device_concessao` conserva geração inteira positiva por chave mesmo depois de `remove_device`. `nova_concessao` fornece um mecanismo único de gatilhos dentro da transação de cada escritor: put/remove, enrollment de executores, promoção/delegação/revogação ACS4 e SQL direto. Put idêntico também inicia uma nova concessão. Rollback desfaz dispositivo e contador juntos. Contador inválido ou esgotado recusa a escrita; não há CAST permissivo nem reparo silencioso. Uma migração posterior não preenche contadores ausentes de aparelhos existentes.

Atualização de chave/origem/grant/escopos encerra a concessão anterior, inclusive quando os valores forem restaurados. Alteração de metadados, tickets e `last_seen_at` não concede capacidade. Renovação monotônica do lease Account conserva a mesma geração; redução/remoção do lease invalida o retrato. O prazo original de um pedido é limite superior: renovar o mesmo grant não amplia esse prazo. Uma nova requisição autenticada na mesma concessão pode capturar o prazo corrente. Lease não cria grant, escopo, ativação ou posse.

A época RAM do dono muda sob o mesmo lock Cloud quando sua identidade muda, inclusive sair e voltar ao mesmo dono. O incremento fica no módulo próprio `epoca_dono`, por CAS forte (`compare_exchange`) com leitura e ordens de sucesso/falha `Relaxed`. Cada chamada termina só depois de um sucesso; em disputa, recalcula a partir do valor retornado pela falha. A soma é saturada: `u64::MAX` não faz wrap, e a guarda existente continua recusando época esgotada. O gancho permanece no mesmo ramo de troca de identidade e sob a mesma trava do status; alteração sem troca de dono não incrementa a época. As APIs do CAS estão disponíveis desde Rust 1.34, anterior ao MSRV 1.88 do Host; a toolchain fixada em 1.99 não substitui esse requisito. Nenhuma capacidade/descrição de ferramenta ou orientação do harness muda por esta correção interna. Ver [contrato oficial de AtomicU64/compare_exchange](https://doc.rust-lang.org/stable/core/sync/atomic/type.AtomicU64.html#method.compare_exchange). Não é persistida como prova; pedidos RAM também não sobrevivem a reinício. A ordem é sempre Cloud → DB → sincronização. Transições `cloud_synced.store` e a admissão final usam a mesma cerca neutra. Cada perda true→false incrementa uma época RAM monotônica, sem wrap; restaurar true não volta à época anterior. A leitura da época só existe enquanto a reserva opaca mantém essa cerca. Account fixa a época na captura e a compara antes do efeito/commit/enqueue; esgotamento da época recusa Account. Caller Local e DeviceSourceLocal continuam sem requisito de sincronização. Não há uma segunda implementação para Chão2B.

## Composição SAAS-04 e saída

`None` não resolve, exige ou concede `engine.accounts`. `Some` recebe somente a Authority SAAS-04 já capturada no domínio de contas, para o mesmo Device e a mesma captura da requisição. Não pode emprestar a Authority de outra chamada nem usar Authority Local para Device. O binding assinado original, posse, recibo, ativação, lease, dono e capacidades continuam necessários; a leitura usa `engine_binding` na Connection recebida. A Authority de contas conserva o transporte original ao atravessar filas, inclusive sua época de sincronização; não o resolve de novo no commit. Some não renova esse transporte antigo após uma reconexão. Uma nova solicitação pode capturar o estado atual da mesma concessão, mas não altera a Authority ou o Arc dos pedidos anteriores.

A igualdade usada pelas operações SAAS-04 continua identificando o vínculo assinado; ela não renova a guarda. Gravações e consultas continuam conferindo o transporte original da operação e o binding antes/depois do callback. Nenhum caminho de Control genérico foi adicionado ao resolvedor de contas.

O canal prepara respostas e reserva capacidade antes das guardas. A última admissão de respostas e eventos é síncrona. Depois que uma saída foi admitida, não se tenta desfazê-la com uma validação ou commit posterior. Isso delimita admissão no canal, não comprova entrega ou leitura pelo aparelho. A implementação não amplia a cobertura dos efeitos executados pelos consumidores; cada consumidor ainda precisa cercar sua própria escrita/efeito.


## Encerramento da assinatura do canal

Uma assinatura conserva o Caller e o prazo originais mesmo quando o canal recebe pedidos novos com lease renovado. Se a guarda recusar evento ou término, o Hub libera seu job/stream e fecha o canal com4011 e motivo constante, sem entregar dados ou end fora da guarda. O cliente existente faz nova autenticação; isso não renova a assinatura antiga nem repete login do provedor. Só uma solicitação nova, autenticada e ainda autorizada recupera o fluxo.

O id do cliente pode ser reutilizado depois de cancelar. A tarefa original só enfileira evento/end e só remove/fecha o job se a identidade da tarefa Tokio ainda corresponder ao AbortHandle registrado. Comparação e enqueue são síncronos; finalização negativa usa vaga pré-reservada e uma trava jobs, sem Store/Cloud/await nela. A guarda de dados mantém Cloud→DB→sync→jobs. Não há validação/commit falível depois de enqueue autorizado.


Requests também conservam a identidade da própria tarefa. Cancelar retira a admissão da resposta, sem abortar trabalho em andamento. Se outra assinatura ou request reutilizar o id, a tarefa antiga pode concluir seu trabalho, mas não responde nem remove/fecha a sucessora. Depois dos awaits e da reserva da fila, a guarda original compara a tarefa, remove o job e enfileira a resposta na mesma seção síncrona Cloud→DB→sync→jobs. Não há janela entre remoção/envio nem renovação de Caller, lease ou épocas do pedido antigo.


Se a Request ainda for dona do slot, mas sua guarda original recusar depois do trabalho e da espera pela fila, o Hub conserva a vaga já reservada e só enfileira Close4011 com motivo constante, removendo o job na mesma seção síncrona. Isso permite nova autenticação sem entregar dados do pedido expirado. Se o id já tiver outro dono, a resposta antiga é descartada e o sucessor não é encerrado. Renovar a concessão ou reconferir o canal não amplia o prazo do pedido antigo nem aborta seu efeito em andamento.

## Ponte MCP

`Caller::Ponte { bot, sessao, origem }` identifica o transporte MCP com credencial por
provisionamento, verificada antes do dispatch HTTP. O bot e o vínculo vêm da
credencial; divergência no corpo é recusada. Não concede Authority do dono,
`engine.accounts`, prova humana ou aprovação; SAAS-04 permanece inalterada.

A lista fechada permite `connectorCall`, `routineCall`, `computerCall`,
`computersCall`, `chatCall`, `teamCall`, `manageCall`, `memoryCall`,
`computerTools`, `donnaGuardRuntime`, `donnaGuardObserve`, `donnaGuardRelease`,
`donnaConectorFrame` e `donnaConectorRelease`. Qualquer outro método é recusado,
inclusive decisões administrativas dos conectores. As aprovações de gestão e de
Times continuam no ingresso Local/Device existente. Fora de `/api`, a Ponte é
recusada, inclusive em `/events`, terminais, `/channel` e `/ingest/statusline`.
Bearer inválido nunca é promovido a Local. Leitores administrativos continuam Local;
a descoberta de setup remoto continua no Hub com seu Setup restrito existente.

O hash fica só na memória do Hub. Rotação por slot, parada e encerramento de sessão
invalidam o uso anterior. Isso não impede um processo com o mesmo UID de ler o token
Local em disco. A linha de status do motor não envia uso ao Hub.

### Porteiro de chamadas MCP

O Hub reavalia as regras Recusar para a chamada e o contexto original. Stdio
revalida no início e antes de fechar o frame; HTTP remoto avalia no Runtime e
o proxy curado, em `before_send`. Falha de leitura recusa. Permissão de sessão
não sobrepõe Recusar; argumentos MCP não comprovam o alvo, de modo que uma regra de alvo pode bloquear amplamente.
`resources/read` e `prompts/get` só obedecem à regra do conector inteiro.
A verificação não é atômica com a escrita: a janela aceita costuma durar milissegundos
em stdio e pode incluir um handshake TLS em HTTP; não há prazo máximo garantido. Uma regra criada nessa janela
pode não barrar a chamada; o envio sob a mesma autoridade da política fica
para uma etapa futura. Isso não cobre terminal ou motor nativo.

### Saída comum e credencial original da Ponte

`com_saida` mantém sua assinatura `FnOnce() -> T` e delega a um único corpo
canônico `com_saida_validada`. A variante recebe a origem já capturada, uma
pré-checagem `FnOnce(&Connection) -> Result<()>` e, separadamente, o envio
síncrono `FnOnce() -> T`, sem Connection. O callback DB usa só a Connection;
não há FS, Store/Cloud reentrantes, await ou envio em `with_db`. Tanto o caminho
anterior Local/Device como o writer/retorno de observação usam esse mesmo corpo;
contas, escopos, lease, sincronização e classificação de falhas de controle
continuam nas guardas existentes.

A autenticação da Ponte conserva por Arc o mesmo objeto Credencial da coleção
canônica; não serializa token/hash/origem nem cria novo registro ou Authority.
Revogar/substituir o objeto invalida o Caller antigo, mesmo com bot/binding/slot
iguais. O envio da tentativa retém a coleção canônica durante a conferência do
binding no DB e o último envio síncrono. A ordem é Cloud → coleção Ponte (só
neste domínio) → DB → sincronização; o consumidor mantém suas guardas externas
memory_boundary/emit/runtime. Revogadores/provisionamento não entram em Cloud/DB
sob a coleção. `Autoridade::capturar` continua recusando Ponte: ela não vira
Local, Control ou `engine.accounts`, nem abre métodos fora de PERMITIDOS.

O consumidor é `computerCall` → Attempt → helper writer/CUA exchange e retorno.
O teste de origem passa por provisionamento e autenticação HTTP reais da Ponte,
dispatch e peer UnixStream. Não é evidência de exclusão de janela nem término
nativo, e a caixa protegida continua indisponível.
