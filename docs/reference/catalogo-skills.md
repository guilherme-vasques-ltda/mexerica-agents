# Catálogo passivo de habilidades

O Hub mantém o catálogo curado. Instalar importa texto verificado e não seleciona a
habilidade em nenhum bot. A seleção continua nas configurações de cada bot. Texto importado
não concede ferramentas, execução, contas, acesso a dados, rede ou autonomia.

## Contrato para clientes

`apps/desktop/src/shared/fork-skills.ts` define os DTOs e o wrapper forte `passiveSkillsV2`. O wrapper legado
`passiveSkills` permanece para leituras V1 compatíveis. O contrato forte usa os RPCs existentes:

- `marketSkills`: request `{skillProtocol:2}`; reply `{skillProtocol:2,items: CatalogSkill[]}`, com versão, origem, licença, perdas e disponibilidade.
- `skills`: request `{skillProtocol:2}`; reply `{skillProtocol:2,items: InstalledSkillV2[]}`; inclui `installationId` opaco do Hub e `contentStatus`; legados e textos manuais têm recibo nulo.
- `skillDetails`: `{skillProtocol:2,id}` instalado ou `{skillProtocol:2,source}` do catálogo; devolve `skillProtocol:2`, texto, metadados, recibo e `installation` (registro completo da encarnação; null para source).
- `installSkill`: `{skillProtocol:2,catalogId,expectedDigest}`. Não aceita URL, manifesto nem revisão escolhida
  pelo cliente. Com `expectedInstallationId` opcional da linha vista, uma curada alterada fora do app é
  reinstalada pelos bytes do pacote do recibo (ver "Recuperação"). Retorna `{skillProtocol:2,skill,alreadyInstalled}`. `source` remoto antigo é recusado.
- `installSkill` manual: `{skillProtocol:2,name,description,instructions}`, dentro dos limites do formato.
- `removeSkill`: `{skillProtocol:2,id,expectedInstallationId,expectedReceipt}`. A identidade e o recibo devem ser capturados juntos da instalação vista. `expectedReceipt` é `{kind:"none"}` ou `{kind:"catalog",catalogId,digest}` exatos. Omissões, extras e o payload antigo `{id,expectedDigest?}` falham com `skill_installation_unsupported`, sem fallback. Reply de remoção: `{skillProtocol:2}`. A remoção não reinstala o seed no próximo boot.

`text` representa pacote textual integral; `adapted` identifica derivado parcial explícito;
`dependencyUnavailable` e `licenseUnavailable` não permitem instalação. `installed` se refere
ao digest atual, e `installedId` identifica o registro real para seleção/remoção. A UI deve
mostrar `reason`, `losses` e `requirements`. O recibo verifica proveniência e integridade,
sem afirmar scanner completo, Guard ou aprovação ISO.

Clientes devem invalidar consultas ao trocar de Hub e após mutações. `SkillRequestScope`
descarta respostas e erros antigos; o consumidor mantém busy e seleção por Hub, bloqueia
cliques repetidos e recarrega catálogo/instaladas após instalar/remover. Não persistir esse
catálogo no espelho: são consultas RPC ao Hub atual, fora de Hello/bots/entries. Por isso
`MIRROR_SCHEMA` não muda.

## Protocolo forte e resultado incerto

O wrapper `passiveSkillsV2` envia `skillProtocol: 2` em TODAS as chamadas, inclusive
consultas e criações. Os parsers antigos de mutação recusam esse campo antes do efeito;
nunca retentar sem ele. Replies novos têm `skillProtocol: 2` no envelope de listas
(inclusive vazias), detalhes, criação/importação e remoção. Uma lista vazia sem versão
não comprova suporte. Detalhes/fichas mantêm os campos de encarnação e recibo existentes.

Requests de listas usam `{skillProtocol:2}`; os demais usam o discriminador nos shapes
acima. Remoção forte continua com id/expectedInstallationId/expectedReceipt capturados.
O marcador é versão, não grant: Caller/contexto/seletores e parsers fechados continuam.
Leituras e criação/importação V1 seguras continuam no servidor; remoção V1 só com condição
forte completa. O wrapper V2 nunca usa essa compatibilidade como fallback.

Recusa comprovada de parser/versão antes do efeito pode ser unsupported. Depois do envio,
reply inválida/sem versão ou falha genérica/perda de resposta é `skill_mutation_outcome_unknown`
(`needsReconciliation: true`): releia o mesmo destino autorizado antes de novo gesto.
Não dizer que nada ocorreu nem repetir criação manual automaticamente. Não há ledger/outbox
novo; o receipt comum de execução pertence ao núcleo.

## Identidade de instalação e compatibilidade

O Hub emite `installationId` estável durante a instalação. Remover/reinstalar sempre
emite outra identidade, inclusive com ID, texto, metadados e digest idênticos. Não usar
nome, hash do pacote, relógio ou caminho como substituto. Lista e install retornam a
mesma projeção; detalhes por ID incluem `installation` da mesma leitura do corpo/recibo.
RPCs distintos podem observar encarnações distintas: o consumidor deve comparar a
identidade da linha com a de detalhes e recusar mistura/resposta tardia.

`passiveSkillsV2` e os DTOs obrigatórios dedicados são a fonte do consumidor novo.
Consulta sem a prova de versão ou recusa conhecida de parser antigo produz
`skill_installation_unsupported`. Reply de mutação já enviada sem essa prova é incerta,
como descrito acima; o wrapper nunca retenta com o payload antigo. O wrapper legado permanece tipado para o
consumidor anterior, mas suas remoções insuficientes são recusadas pelo Hub novo.

Exemplo sintético: `{"skillProtocol":2,"id":"manual-exemplo","expectedInstallationId":"11111111-1111-4111-8111-111111111111:7","expectedReceipt":{"kind":"none"}}`.
`skill_installation_stale` exige nova leitura e novo gesto; nunca recapturar o alvo só
para retentar. A ausência com pedido estrito pode ser idempotente, sem limpar órfãos;
reinstalação posterior recusa a intenção velha. A condição não concede permissão.

A baseline de corpo de um pacote usa o hash de SKILL.md do recibo já preservado;
a migração não legitima bytes adulterados antes dela. Sem recibo, só uma expectativa
anterior preservada permite conferir a manual/legada; ausência de prova não adota o
arquivo corrente. Recibo sem hash de SKILL.md fica unavailable.

O metadado de encarnação fica em `donna.passiveSkills.v2`, no MESMO Store/domínio, com
namespace e sequência persistidos. O v1 mantém só seedDone/receipts/installed para o
loader antigo. A migração transporta os IDs/baselines exatamente e publica v1/v2/guards
na mesma transação, sem apagar autoridade forte para contornar recuperação. A inicialização legada escreve o metadado sem regravar o registro
`skills`, corpo, recibo de disco, envelope, seleção, pin, arquivo ou seed. Registro e
identidade de instalação nova são gravados na mesma transação; a remoção revoga vínculos
na transação da ausência, antes da limpeza dos arquivos. Falha de limpeza deixa órfão
privado, sem instalação ativa; não repetir efeitos por resultado incerto.

`contentStatus` é `verified`, `changed` ou `unavailable`. A integridade compara registro/
recibo e bytes limitados de SKILL.md sob os locks dos escritores do Hub. Corpo alterado
externamente não recebe versão nova por leitura: detalhes recusam com
`skill_installation_changed`; arquivo irregular/ilegível recusa com
`skill_installation_unavailable`. A remoção confere encarnação, recibo e registro, não o
corpo. Legado irregular ganha identidade estável sem reparo
ou reimportação automática. Não se promete detecção perfeita de escrita que ignora os
locks ou restaura bytes entre verificações, nem auditoria de todos os arquivos externos.

Entrega ao motor ("bloquear e avisar"): `skills_brief` e a expansão de `/id`
chamam o mesmo `checked_document` para cada skill ligada e instalada, no instante da
montagem. `verified` segue igual a antes; `changed` e `unavailable` não são anunciados
nem expandidos com caminho, e o `/id` recebe só um aviso sem caminho. O texto manual usa
o `body_digest` da própria instalação. O registro é lido sem lock de arquivo, migração ou
escrita: sem metadado v2, marca de escritor legado, guarda ausente ou identidade
incompleta, nenhuma skill é anunciada e o bot recebe o motivo, até a próxima leitura sob
os locks (inicialização do Hub ou lista do app). O vínculo `cfg.skills`, o arquivo e o
registro não mudam; nada é restaurado, reinstalado ou desligado sozinho. O app mostra
"Alterada fora do app" pelo `contentStatus` existente. Limites: a conferência não cobre
leitura posterior pelo mesmo caminho (TOCTOU), sessão que já conhecia o caminho, o que o
motor leu de fato nem snapshot imutável. Não é scanner
nem dá confiança ao conteúdo.

Recuperação por gesto do dono (nada automático):

- **Remover** alterada ou ilegível: `removeSkill` com a encarnação e o recibo vistos.
  `record_intact` exige o registro emitido; encarnação/recibo divergentes recusam com
  `skill_installation_stale` e registro divergente com `skill_installation_changed`, sem
  efeito. A revogação dos vínculos e a limpeza seguem o fluxo de sempre.
- **Reinstalar curada**: `installSkill {catalogId, expectedDigest, expectedInstallationId}`.
  Só quando o recibo instalado é o do pacote atual do catálogo, a encarnação é a vista e o
  registro está íntegro. O Hub monta os bytes do pacote embutido (nunca do disco ou do
  recibo de disco) num stage verificado e troca a pasta por rename atômico
  (`RENAME_EXCHANGE`/`RENAME_SWAP`); destino irregular recusa. Depois de gravar, confere
  `checked_document` e os arquivos do pacote: só então responde sucesso, na mesma
  encarnação. Falha depois de gravar responde erro e a skill continua bloqueada. Sem a
  encarnação, o pedido segue recusando com `skill_installation_changed`. Recibo de versão
  anterior do catálogo não é reposto por aqui.
- **Texto manual**: não há restauração nem recibo. O dono remove e salva o texto de novo
  pelo fluxo manual; é outra encarnação, sem religar bots sozinho.

### Fundação manual

O salvamento humano existente (`installSkill` manual) passa a emitir `manualRevision:1`
no registro interno da instalação, junto do `bodyDigest` calculado sobre os bytes
canônicos do formulário já validados em memória. Depois de publicar, o Hub confere o
arquivo contra essa expectativa antes de persistir; não relê o arquivo para criar a
base aprovada. Falha não registra a manual e limpa sua publicação. O cliente não
escolhe revisão ou digest manual; o marcador não é recibo de curadoria, autoria,
licença ou certificado de segurança. Não há campo RPC/mirror ou ação nova.

Lista, detalhes, perfil e `/id` continuam usando o mesmo verificador e os mesmos
bytes de índice/intenção. Leituras não emitem revisão. Uma manual v2 já conferível
continua disponível com seu digest anterior e revisão desconhecida, sem fabricar
revisão 1 retroativa. Isso não comprova o gesto original nem remedia um digest que
um escritor anterior já tenha recapturado do disco.

Na reemissão por escrita antiga ou guarda ausente, o Hub preserva a expectativa
anterior somente se o digest do registro ainda corresponde. Nunca calcula nova
aprovação a partir do corpo corrente. Mantém a rotação de encarnação anti-ABA; corpo
adulterado continua `changed`. Sem prova preservada ou com registro incompatível,
a nova encarnação fica `unavailable`, sem apagar corpo ou vínculos. A lista ainda
permite capturar a condição atual para **Remover**; salvar de novo continua gesto
separado, com outra instalação, sem religar bots. Estrutura v2 inválida continua
falhando fechado pelo contrato existente.

Não injeta corpos inline nem aumenta o prompt. Curadas, reinstalação, autoridade e seleção não mudam.

Restore pertence ao fluxo de backup/memória: restaurar DB/estado e arquivos coerentemente e invalidar
intenções anteriores/reemitir identidades antes de reabrir clientes. Snapshot antigo pode
ressuscitar IDs; não há proteção global contra rollback e restore real neste contrato.
Não há Store, ledger, ator, fila ou outbox paralelos. Campos RPC não mudam MIRROR/hello.

## Medida do índice

A leitura `skills` é estendida em vez de criar RPC. `skills {botId}` (só `skills`; `marketSkills` continua sem corpo)
devolve a lista de sempre e `index: {enabled, listed, bytes, selectionVersion}` daquele bot, ou
`index: null` quando o registro não pôde ser conferido. Pedido vazio mantém o contrato anterior,
sem `index`. A guarda de Space e `require_current` de sempre valem na entrada e depois dos locks;
na leitura efetiva, a linha do bot é conferida de novo (viva e do Pessoal) antes da lista e da
medida. Bot de outro Space, apagado ou inexistente, conversa, campos extras e Hub/contexto
inválidos continuam recusados antes de ler o catálogo (SPACE-SKILLS-1); o caso do bot vivo do
Pessoal passou a teste positivo.

A medida sai da mesma montagem do bloco de `skills_brief` (`entrega::montar`): `enabled` são os
IDs distintos de `cfg.skills` (inclusive ligada não instalada), `listed` as linhas anunciadas
depois de `checked_document`, `bytes` os bytes UTF-8 exatos do bloco com a frase fixa do início
(0 sem bloco). Registro não conferido: o bot recebe o aviso de sempre e a medida é `null`, nunca
zero. `selectionVersion` é a versão (`donna_skills_version`) da mesma linha lida; o app só
mostra a medida quando ela é a da seleção na tela. É só resposta de RPC: não entra no espelho
nem no cache, por isso `MIRROR_SCHEMA` não muda. O texto entregue ao motor não muda, a medida não
corta nem limita nada e não mede corpo de SKILL.md, conversa, tokens, o que o motor leu nem
sessão anterior. Um Hub anterior recusa o `botId` com "Intenção de consulta de habilidades
inválida"; só essa recusa, com a mesma origem viva, faz o app reler sem `botId` e mostrar a
medida como indisponível. Qualquer outra falha segue como erro, sem segunda leitura.

## Escopo Pessoal e seletores de contexto (SPACE-SKILLS-1)

O catálogo e o registro atuais pertencem exclusivamente a **Pessoal**. `skills`,
`marketSkills`, `skillDetails`, `installSkill` (catálogo ou manual) e `removeSkill`
recusam um Space adicional com `skills_personal_only`, antes de consultar o registro,
projetar itens/contagens/installed metadata ou alterar arquivos/vínculos. Uma recusa
não representa catálogo vazio. Skills por domínio continuam planejadas e indisponíveis.

O legado sem seletor conserva o destino Pessoal. O cliente pode enviar `spaceId` de
Pessoal explicitamente; o dispatch autentica o transporte e valida Caller, Hub, Space e
referências pelo guard comum antes da adaptação. Um pedido cujo bot/conversa/Hub
conflite com Pessoal é recusado. Seleção de contexto não é credencial nem grant.

A adaptação interna `PersonalSkillRequest` conserva o contexto original para revalidação
e retira somente o `spaceId` já comprovado como Pessoal do payload entregue aos parsers
estritos existentes. Não retira seletores de bot/conversa nem campos desconhecidos;
esses campos continuam recusados. Listas aceitam só payload vazio, e detalhes exatamente
`id` ou `source`. Os tipos de intenção mantêm `deny_unknown_fields`. O `hubId`, quando
presente, segue a validação e adaptação comum do dispatch. A adaptação de contexto não altera o DTO de domínio; a evolução de identidade acima é dedicada ao catálogo.

O limite único de marketplace encaminha os métodos ao módulo do catálogo. Nas leituras,
os três wrappers existentes de market recebem obrigatoriamente o callback síncrono
de Caller/contexto original; o domínio o executa sob MUTATION e lock de filesystem
antes de load, migração ou projeção, sem await entre revalidação final e leitura. A autorização
atual e o contexto são revalidados antes de dados, após aguardar `team_lock` e nos pontos
existentes de revalidação de mutações. Não há fallback de outro Space para Pessoal.
Clientes conservam a conexão/generation capturadas no gesto; reconexão ou ABA exige novo
gesto, sem recapturar intenção automaticamente. Uma operação enviada pode ter concluído:
releia o mesmo destino autorizado antes de repetir resultado incerto.

## Vídeo

A receita textual HyperFrames é separada de capacidade operacional. Readiness permanece
`enabled: false`, `runtime: unavailable`, com motivo e ações indisponíveis. A importação
não executa probe, download, CLI, render, preview, Studio, registry, API ou modelo.

## Integridade, publicação e limites

O catálogo desta revisão é um bundle incorporado ao Hub, com 29 entradas:
28 externas e a skill autoral passiva `criador-de-skills`. Browse/details e
install não consultam GitHub, tags, branches, npm ou serviços. Não existe importação remota
arbitrária: o caminho antigo `source` é recusado mesmo quando combinado com uma intenção
nova. Clientes antigos precisam usar o contrato curado para instalar; ler legados continua permitido; remover exige a condição de encarnação nova. Texto manual permanece disponível sem importar pacotes ou URLs.

O envelope SHA-256 usa JSON UTF-8 compacto com chaves ordenadas recursivamente:
`{metadata, installId, files}`. `metadata` exclui seu próprio `digest`; `files` repete o manifesto
ordenado da versão. Tamanho, modo original `100644` e SHA-256 de cada conteúdo são conferidos.
`sourceDigest` identifica o pacote original da auditoria de procedência, cuja canonização
pode diferir; nunca substitui o digest do envelope importado. Repositório, commit e pasta
originais ficam no recibo. Derivados incluem `ADAPTATION.md`, revisão, licença e perdas.

O Hub valida o envelope e o parser Agent Skills antes de registrar. Limites: SKILL.md até
32 KiB, arquivo até 1 MiB, pacote até 5 MiB/100 arquivos e profundidade de quatro diretórios.
Somente Markdown, texto de licença e JSON passivo são aceitos pelo bundle; sem links,
executáveis, arquivos especiais, scripts ou HTML ativo. O cliente não escolhe o manifesto.
A importação usa staging privado, arquivos novos sem bit executável, conferência de conteúdo,
rename e transação de registro. Locks de processo e filesystem serializam mutações. Não há
pacote parcial visível no cadastro nem sobrescrita de versão existente. Repetir o mesmo
id/digest confere os bytes e retorna a instalação existente. Digest diferente exige escolha
explícita; não existe atualização automática por main/tag nem atualização durante o uso.

Interrupção antes da publicação deixa apenas staging privado; uma falha normal o remove.
Após rename e antes da transação pode restar um pacote completo ainda não cadastrado. Uma
nova tentativa só o recupera com todos os arquivos e recibo idênticos, sem sobrescrever.
Falha de transação remove o pacote recém-publicado quando possível. Uma pasta irregular ou
com outro conteúdo é preservada e a intenção falha. Exclusão remove cadastro e vínculos
na transação antes de limpar arquivos; se a limpeza falhar, o erro informa essa pendência.
Os recibos mínimos permanecem indexados pelo digest após remoção, sem guardar os corpos.

Find-skills é pré-instalada uma vez, com o texto original no pin fixado. O marcador durável e a remoção
intencional impedem que o próximo boot o traga de volta. O seed não seleciona bots. Bots novos
começam sem skills por padrão; escolhas explícitas e vínculos legados salvos são preservados.
Remover revoga o vínculo correspondente, para que reinstalar não reative uma escolha antiga.
O mecanismo de criação de bot pelo Berga continua herdando somente a seleção explícita do
criador, conforme a política vigente; não herda todo o catálogo instalado.

## Procedências e disponibilidade

A correção LANG-1 conserva nomes, identificadores de origem, descrições e instruções no
idioma original, inclusive nas entradas indisponíveis. IDs de catálogo são chaves internas
estáveis e podem diferir do identificador original (por exemplo, orca-reference/orchestration).
Controles, motivos e avisos próprios da Mexerica continuam em pt-BR. Texto manual criado em
português continua em português. Nenhum arquivo instalado é traduzido ou regravado.

Doze entradas contêm SKILL.md original integral, entre elas security-audit, impeccable,
frontend-design, find-skills, grilling, research, brainstorming, content-strategy,
marketing-psychology, hyperframes e stop-slop. O rótulo
adapted identifica embalagem com texto original e aviso donna-original-text-v2.
Stop-slop usa a embalagem passiva de marketing; copywriting e copy-review-panel têm adaptações
textuais declaradas (ver Marketing passivo abaixo). Frontend-design, content-strategy e marketing-psychology mantêm os pacotes
textuais integrais anteriores. Os manifests enumeram tudo que é distribuído e as omissões.
Recursos gráficos, fontes, scripts, runtimes e executores não são distribuídos.

Quatorze entradas ficam indisponíveis com causa concreta. PDF/XLSX mantêm a restrição de
licença e Orca permanece referência sem runtime. Copywriting é adaptação instalável,
com o auxiliar de origem incerta excluído e substituído por guia curto autoral declarado. Onze incompatibilidades adicionais preservam o original sem tentar adequar o YAML:
Hallmark/handoff usam campos extras; skill-creator excede 32 KiB; Browser Use usa homepage e
metadados estruturados; quatro guias Firecrawl build/deep usam inputs/references; três guias
Firecrawl core usam allowed-tools como lista. Essas onze são dependencyUnavailable,
indisponíveis no importador atual, e não falhas de licença. A compatibilidade posterior
tem escopo próprio; este saldo não
encerra o catálogo amplo nem autoriza mudar o parser.

`source-manifest.json` no módulo do catálogo liga cada arquivo copiado ao path original,
commit, Git blob, SHA-256, tamanho, linhas, licença efetiva e aviso. Nada consulta upstream
em runtime. Instruções para CLI, delegação, instalação ou API podem constar do texto original;
são dados passivos e não criam ferramentas, permissões, runtime, conta, rede ou autoridade.
HyperFrames continua desligado e operacionalmente indisponível. Nenhuma importação faz probe,
instala SDK, executa setup, ativa serviço ou provisiona máquina.

## Embalagens anteriores e recibos

Reparar metadados/bytes do bundle exige novo digest de envelope, mantendo o mesmo pin e
sourceDigest upstream. Não é uma nova versão upstream, atualização automática ou upgrade de
instalação. Novas importações usam o identificador original como destino; ID de catálogo e
nome de exibição nunca autorizam sobrescrever ou renomear uma pasta existente.

`legacy-envelopes.json` captura os 26 envelopes anteriores sem seus corpos e serve apenas
à leitura de recibos por catalogId+digest. Não é uma segunda lista instalável. Os detalhes
de uma instalação antiga usam seus metadados capturados, arquivo existente e recibo exato;
a ausência futura de um catálogo não apaga a proveniência do recibo. Um recibo divergente
de qualquer envelope conhecido continua visível, sem atribuir metadados de outra versão.
A remoção continua por ID instalado e digest do recibo, incluindo versões antigas.

Um recibo prévio de find-skills impede nova tentativa automática de seed mesmo que o
identificador novo seja diferente. Browse/details/seed não regravam pacote, recibo, nome,
pasta ou seleção anterior. Uma nova importação explícita com digest atual pode coexistir
com a antiga se o destino original estiver livre; no mesmo destino, exige remoção explícita
prévia. Nomes iguais em IDs diferentes não são equivalência nem substituição.

Se uma identidade de instalação já pertencer a um legado ou outra versão, o catálogo
explica o conflito e não oferece importação sobre ela. O seed não declara esse legado como
sua versão: preserva-o e registra que a tentativa inicial foi feita. Respostas de detalhes
também devem ser invalidadas ao trocar a seleção na UI, além da troca de Hub e das mutações.

## Orientação para o motor e manutenção

Nome/descrição originais acompanham o cadastro da nova importação. Somente IDs selecionados
explicitamente para o bot entram em `market::skills_brief`, consumido pelo perfil dinâmico
em `chat/context.rs`; o perfil informa nome, descrição e o caminho real de SKILL.md.
A expansão `/id` em `chat/prompt.rs` pede leitura desse arquivo como orientação externa,
sem conceder ferramentas ou autoridade. O corpo original é lido sob demanda: instalar não
anexa automaticamente os pacotes ao prompt nem ativa a skill no bot. Uma entrada bloqueada
não cria cadastro/arquivo novo e não deve ser prometida como ferramenta pronta.

Indisponibilidade prospectiva do catálogo não apaga nem desliga uma versão anterior já
instalada. Os detalhes/recibo e o perfil dessa instalação continuam usando o registro e
os arquivos capturados; o catálogo atual não renomeia, traduz ou atualiza seu conteúdo.
Instalar texto que menciona CLI/API/delegação não disponibiliza essas capacidades.

Os RPCs documentados acima são ações do cliente/Hub; esta correção não anuncia ao motor uma
nova ferramenta MCP de catálogo. As descrições das skills no bundle e no demo são originais;
os motivos, limites e erros próprios permanecem em pt-BR. Não reescrever a fonte para que ela
pareça o manual da Mexerica: a regra de autoridade continua no Hub e no manual estável.

Esta rodada atualiza também a seção Skills de
`docs/features/marketplace.md` e o guia embutido `docs/features/manual-do-harness.md`,
removendo a afirmação anterior de ativação automática. `host/src/agent/harness.rs` permanece
byteigual: sua orientação estável não afirma ativação de todas as skills instaladas; o
fluxo existente instala os bytes novos do guia embutido. A responsabilidade ampla continua
com o dono, que deve consumir esta revisão; fixtures de contexto/slash não comprovam
uma sessão real de provedor ou o app instalado.


## Recuperação de escritores antigos

Guards SQLite locais filtrados a skills/v1 marcam o v2 na transação de escritores
antigos. O dono forte verifica definições e v1/v2/marker antes do primeiro put, não
limpa marca tardia com snapshot velho, e finaliza registro/recibos/identidades/vínculos
atomicamente. Guard ausente com v2 exige reatestação; definição/layout inesperado falha
fechado, sem substituir guard arbitrário.

Após escrita fora do protocolo ou lacuna, reatesta revisões com namespace novo e contador
preservado/avançado antes de dados/efeito e antes de seed devolver o gate de startup.
Isso invalida intenções antigas mesmo com corpo/metadados/digest idênticos. Não afirma
reinstalação física. Leituras/restart fortes limpos conservam IDs. Se a migração
recriar campos fortes no v1 e o v2 estiver marcado, aposenta ambos os conjuntos de IDs e carrega o máximo
dos contadores antes de finalizar uma autoridade só no v2. Dual sem marca/forma desconhecida
recusa. Não regrava corpos/recibos/seleções por esse processo nem altera política do seed.

A API do binário antigo continua com seus limites; não há garantia forte instalada nele.
Restore físico/rollback de DB que repõe IDs/guards pertence ao RecoveryEpoch/restoreEpoch
único e ao fluxo de backup/memória. Não há ledger paralelo por esta migração.

## Skill autoral passiva de extensibilidade

O mesmo catálogo inclui `criador-de-skills`, síntese autoral própria; a entrada
Anthropic `skill-creator` permanece indisponível e intacta. Seu `Package` é
construído do JSON passivo gerado dos Markdown canônicos, validado pelos limites existentes
e identificado pelo digest de envelope e pelos hashes/tamanhos dos arquivos.
A origem aponta o guia fixado em `docs/guides/extensibilidade/`; `ADAPTATION.md` explicita o texto
autoral e os capítulos da build atual. Não há fetch nem atualização por branch.

O startup do manual projeta somente esse pacote público em
`docs/extensibilidade/criador-de-skills/` sob a área de dados do Hub. Usa a mesma
regra de leitura da documentação no perfil restrito; não concede escrita,
ferramenta, acesso ao catálogo pessoal ou a outros arquivos/skills. O manual
estável contém só a orientação e o caminho real; `SKILL.md` e `references/`
são lidos conforme o pedido. Não há registro de instalação, seed novo, seleção
para bot ou corpo de skill anexado a todo turno por essa projeção documental.
Os RPCs existentes continuam sendo do cliente; nenhuma ferramenta de catálogo
é acrescentada ao bot. Instalar explicitamente pelo app continua operação
separada de selecionar para bot e não decorre desta leitura.

Qualquer bot pode recomendar e preparar rascunho para o dono; não
instalar. A revisão aprovada abrange os bots que usam a skill, sem operação de
edição/publicação global. O segredo por referência
permanece no manual, na descrição e na skill. Não há verify MCP,
importação de Plugins de IA, execução ou permissões adicionais.

A fonte legível permanece em `docs/skills/criador-de-skills/` e nos seis
capítulos do guia. Antes de publicar alterações desse conteúdo, execute
`node host/src/fork/skill_catalog/gerar-criador-de-skills.mjs`, confira com
`--check` e inclua `host/src/fork/skill_catalog/criador-de-skills.json` na mesma
mudança. Somente esse JSON dentro de `host/` é incluído no Rust; os novos Markdown
não são entradas diretas de compilação. Assim o artefato compilado passa pelas
verificações existentes sem mudar hook/CI. A geração é local ao desenvolvimento, sem
rede, scripts no pacote passivo, build.rs ou executor no Hub.

## Comparação manual

`skillDetails` admite a opção estrita `review`, protocolo 1, para comparar uma
manual com prova de revisão. Não salva revisão, publica arquivo, aprova ou
emite recibo. A leitura normal anterior permanece compatível. A UI prepara um
rascunho e exibe documentos integrais e bots afetados; não há aplicar/aprovar.

Pedido: `{id, review:{protocol:1, expectedInstallationId,
proposal?:{expectedBodyDigest, markdown}}}`, com os mesmos seletores Pessoal/Hub
e protocolo 2. Sem `proposal`, retorna base integral conferida para iniciar o
rascunho. Com proposta, exige o digest dessa base e valida SKILL.md integral
pelo parser atual (até 32 KiB, `name` igual ao ID). Não gera outro ID por título.
Campos extras, protocolo inválido e proposta malformada são recusados antes de
ler catálogo ou arquivos. Curadas não são editáveis; manual v2 sem revisão
continua legível normalmente, mas o exame informa base desconhecida, sem migrar
ou legitimar bytes, nem sugerir remoção automática.

Resposta: `{skillProtocol:2,review:{reviewProtocol:1,effect:"none",id,
installationId,base:{revision,bodyDigest,markdown},proposal:{revision,bodyDigest,
markdown}|null,affectedBots:[{id,name,selectionVersion}],comparisonDigest}}`.
`markdown` inclui frontmatter e corpo sem truncamento. A revisão da proposta é
apenas a próxima candidata; nada é persistido. O digest usa SHA-256 do JSON
UTF-8 compacto com chaves ordenadas recursivamente de `review`, excluindo
`comparisonDigest`. Não é credencial, aprovação, recibo ou reserva de versão
ou alcance para um commit futuro. Comparar novamente relê o alcance atual.

O alcance deriva exclusivamente de `cfg.skills` dos bots vivos, ordenado por ID,
com versões para evidenciar ABA; duplicata de vínculo não duplica bot. Base e
alcance são conferidos na mesma Connection contra o snapshot do catálogo, sob
locks e autoridade original capturada pelo produtor existente. Guards Caller e
Pessoal são conferidos antes de dados e ao responder. Callback da autoridade
só consulta a Connection recebida; leitura de arquivo fica fora dele, com os
bytes verificados contra a expectativa anterior. Falha de alcance não vira lista
vazia. O exame não migra, reemite identidade, limpa reemissões ou altera seleção.

Erros próprios: `skill_review_invalid`, `skill_review_manual_only`,
`skill_review_base_unknown`, `skill_review_base_changed` e
`skill_review_scope_unavailable`; erros de instalação e Caller/Space continuam.
Rascunho alterado invalida comparação da UI. Troca de Hub/conexão ou falha conserva
rascunho sem apresentar resposta antiga como atual; nenhum efeito é repetido.

**A leitura operacional pelo motor permanece bloqueada**: ACP anuncia `fs.readTextFile:false`;
o motor lê por ferramenta nativa sem revalidar `cfg.skills`, e `/id` já pode
estar expandido antes da fila. Metadados de comparação não fecham TOCTOU,
snapshot por turno ou revogação de sessão antiga.
Nada injeta todas as skills inline, amplia prompt/custo, concede
ferramentas ou promete apagar conteúdo já lido. Caminho imutável sozinho não
resolve revogação. Pacote, instalação e sessão real não são validados pela comparação.

Queda ou novo hello no mesmo Hub preservam só o texto/alvo em memória da
abertura, sem base, resultado ou pedido automático. Reler a instalada exige
novo gesto na mesma instalação; outro Hub ou instalação não adotam o rascunho.
Fechar, Voltar, abrir outra linha ou sair da página Skills descarta o texto.

## Marketing passivo

`copywriting` é adaptação no mesmo pin já usado pelo produto; exclui integralmente
o auxiliar de licença não comprovada; seu link aponta agora para o guia curto
autoral `references/donna-transitions.md`, independente da fonte excluída. Conserva os
demais textos e créditos MIT, com seção local declarada de limites. `stop-slop`
traz SKILL e três referências originais, com embalagem passiva declarada.
`copy-review-panel` deriva somente o método textual MIT de copy.md do marketing-os,
com nome e descrição próprios. O pacote marketing-os completo continua piloto futuro.

São três opções passivas no catálogo existente: instalar e ligar a cada bot
continuam gestos separados do dono. Listar ou ler detalhes não instala nem
seleciona; instalações anteriores não são sobrescritas ou atualizadas. Sem bots
novos, subchamadas, ferramentas, scripts, serviços, escrita de relatórios ou
publicação automática. A autoridade verificada no Hub permanece.

O painel usa cinco perspectivas numa resposta: cético, estranho/leitor frio,
concorrente, comprador e editor; cada uma 0–20, total 0–100. Abaixo de 70,
refazer/descartar; 70–84, revisar a principal objeção e conferir de novo; >=85,
candidato para apresentar ao dono. Até três rodadas. A nota é julgamento
editorial heurístico; não comprova conversão, qualidade, segurança ou decisão
do dono. Fatos, métricas, depoimentos e promessas exigem evidência real.

Pins, alterações e avisos por arquivo estão em [THIRD_PARTY](../../THIRD_PARTY.md)
e no manifesto de origem. O sourceDigest upstream de copywriting permanece;
envelope e hashes dos assets identificam os bytes da adaptação separadamente.
As 29 entradas compreendem 12 textos SKILL originais integrais, duas adaptações
textuais, 14 indisponíveis e a skill autoral criador-de-skills. Inclusão
no bundle não comprova instalação ou comportamento em sessão real.


### Preparo interno coerente de Skills no Hub

No Pessoal sem contexto de Space, o preparo conserva a reserva original (Ticket,
origem e destino), seleção, instalação e bytes conferidos no Hub. Cada montagem
nova mantém a conferência dos arquivos e compara a observação original;
divergência retira os caminhos do bloco novo afetado e avisa, preservando o turno.
Perfil enviado e perfil registrado pela sessão usam o mesmo bloco; `adopt` não
reconsulta Skills depois do await para declarar outro perfil como anunciado.

Essa garantia é de preparo interno. Em SystemPrompt, new/load já podem ter enviado
caminhos antes de uma alteração posterior. Nos motores inline, o par preparado
não é recibo de entrega. Cache congelado, handoff, /id pré-expandido, texto raw e
params de Pending permanecem fora da garantia de atualização na entrega. Pending
conserva o preparo; recuperação sem os bytes originais só monta aviso, sem adotar
uma geração positiva para a reserva antiga. Reserva nova pode preparar de novo.
Corpos não são inseridos nos params nem persistidos; memória dura até encerrar o
Ticket (pausas prolongam a retenção, até 256 KiB por documento selecionado).

Caminhos nativos continuam mutáveis. Não há mediação ACP, leitura revogável,
contenção comprovada ou API aceita para a Skills. A leitura pelo motor continua bloqueada;
FD/mmap e a ordenação da entrega seguem em aberto.

### API Hub candidata de leitura mediada

A leitura mediada implementa um consumer ACP privado por skillId, sessão
e attempt original: `_mexerica/skills/read`. Os bytes são os conferidos no preparo,
retidos no Hub. Cada tentativa de entrega revalida seleção, instalação/catálogo,
reserva original e execução ativa na mesma ordem da escrita não bloqueante; commit
DB de revogação não depende de Reconfigure. Não recebe path nem lê FS por pedido.

É API **candidata**, sem ativação operacional. fs.readTextFile segue false;
índice e /id continuam legados. Deny nativo e FD/mmap/dirfd continuam
dependências reais. Conteúdo já recebido/copias/contexto não se revoga.
A rota candidata também registra a exposição da sessão de memória antes
de escrever o corpo/prefixo, sem depender de tool_call. Essas marcas existentes
não são uma nova autoridade e não são apagadas na revogação da skill.

### Lifecycle da entrega candidata

A parada já processada no Pessoal grava `stopRequested` no despacho canônico do
mesmo Ticket, antes do ACK do motor: o token de memória pode continuar válido,
mas nenhuma nova leitura dessa tentativa é admitida. A entrega pendente conserva
Prepared/origem originais em tarefa pertencente à conexão; não bloqueia o select
do ator. Parada/fim interrompem o I/O, e fechamento/prazo/Shutdown cancelam e
recolhem essas tarefas antes de fechar o transporte. Sem bytes escritos, a recusa
preserva stdin para `-32602/skills_read_unavailable` no mesmo canal. Prefixo real
interrompido fecha stdin também no Drop do futuro; nunca continua outro frame.
Não se apagam bytes já recebidos. Essa mudança não altera a política geral de
memória nem cria autoridade, seleção, índice ou ledger paralelo.

A resposta legítima a um cartão ACP da mesma sessão também retorna ao controle
quando aguarda uma entrega candidata no stdin: o mesmo futuro, com ID e opção
já validados, pertence ao JoinSet da conexão. Seu primeiro poll conserva a ordem
FIFO do mutex; não intercala outro quadro após prefixo parcial. Parada, conclusão,
prazo e Shutdown continuam atendidos sem o leitor voltar a drenar. As validações
contextuais e a autoridade original permanecem; responder ao cartão não concede
nova leitura nem reabre um turno.
