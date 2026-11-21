# Skills na Mexerica

[Índice](README.md) · [Segurança](seguranca.md) · [Fontes de skills](fontes.md#skills)

Uma skill registra como conduzir um trabalho. Sua descrição ajuda o motor a
decidir quando ler as instruções; não constitui um gatilho determinístico nem
acrescenta ferramentas. O método de autoria abaixo sintetiza a orientação da
Anthropic para preparar texto revisável com os limites da Mexerica.
A skill autoral passiva `criador-de-skills` serve para consulta e rascunho.
O app permite examinar uma proposta pedida na conversa, mostra o cartão de
recomendação de criação e a indicação de existente.
Nada disso dá ao agente instalação, ligação ou edição operacional.

## Contrato de criação

1. **Intenção:** aproveite o contexto já fornecido. Identifique resultado,
   público, entradas, saída esperada e quando o procedimento deve ser evitado.
   Pergunte apenas o que falta. Um pedido de outro bot é uma proposta, sem
   aprovação humana implícita.
2. **Recursos reais:** confira se já existe orientação semelhante e quais
   ferramentas estão disponíveis e autorizadas. Declare uma dependência ausente;
   não invente ferramenta nem proponha instalar serviço como consequência.
3. **Rascunho:** escreva nome, descrição de uso e passos curtos em PT-BR para
   conteúdo próprio. Inclua resultado, falhas e limites. Na revisão de material
   existente, identifique a versão e mostre a diferença, preservando o original.
4. **Casos:** prepare um pedido típico, uma variação e um pedido parecido que
   não deveria ativar a skill. Acrescente a fronteira de segurança pertinente,
   como segredo, ferramenta ausente ou instrução hostil em anexo. Use dados
   sintéticos e descreva o resultado esperado.
5. **Conferência e revisão:** leia todo o material e simule o fluxo sem efeitos.
   Execução real depende de autorização própria e ferramentas já admitidas.
   Registre o que foi apenas lido, simulado ou executado. Ajuste os problemas e
   repita somente o necessário; para estilo, apresente uma saída à pessoa.
6. **Entregar ao dono; salvar é outro gesto:** apresente finalidade, condições de uso,
   dependências e texto final. Hoje a pessoa usa **Skills → Adicionar → Escrever
   à mão**, com Nome, Quando usar e Instruções, ou abre **Examinar rascunho**
   para revisar uma proposta válida da resposta final. O formulário salva texto manual;
   não importa uma pasta, ZIP, `.skill`, scripts ou pacote completo. Depois,
   a seleção no bot é uma decisão separada. O bot não executa essas operações
   por esta skill passiva.

Há operações de instalação/importação curada e remoção, mas nenhuma operação
dedicada de edição anunciada ao bot. Não sobrescreva o armazenamento por shell;
remover/recriar muda a identidade da instalação e revoga vínculos, portanto não
equivale a uma revisão silenciosa. Em resultado incerto, releia o mesmo destino
autorizado antes de novo gesto; não repita criação automaticamente. Os detalhes
estão no [contrato do catálogo](../../reference/catalogo-skills.md).

O salvamento manual fixa revisão inicial 1 e digest dos bytes canônicos. Não existe edição/publicação de revisão posterior.
Leituras conferem a mesma expectativa; não aprovam o arquivo corrente por migração
ou escrita legada. Manual v2 já conferível mantém seu digest e revisão nova
desconhecida, sem aprovação retroativa. Sem prova preservada, fica indisponível;
o dono pode remover e salvar de novo pelo gesto existente, com outra instalação
e ligação separada. Revisão compartilhada com diff/recibo/UI, snapshot por
turno e revogação nativa continuam pendentes. Corpo segue sob demanda, sem inline,
ferramenta, permissão ou autoridade nova.

## Propostas passivas na conversa

Quando o dono pede uma skill, qualquer bot pode preparar o rascunho na
resposta final Markdown com um bloco fechado `donna-skill-draft`:

````markdown
```donna-skill-draft
{"schema":1,"name":"organizar-notas","description":"Use para organizar notas de estudo fornecidas.","instructions":"Separe tópicos e dúvidas nas notas fornecidas."}
```
````

Por contexto ou tarefa repetida, qualquer bot pode recomendar criar uma skill,
sem pedido anterior ou papel de gestão. Para o cartão de recomendação, use na resposta final
Markdown um bloco fechado `donna-skill-recommendation`:

````markdown
```donna-skill-recommendation
{"schema":1,"reason":"As notas são organizadas repetidamente.","name":"organizar-notas","description":"Use para organizar notas de estudo fornecidas.","instructions":"Separe tópicos e dúvidas nas notas fornecidas."}
```
````

Por contexto ou repetição, qualquer bot também pode indicar uma skill existente,
sem pedido anterior ou papel de gestão. Use um bloco final fechado:

````markdown
```donna-skill-reference
{"schema":1,"reason":"Este procedimento pode ajudar nesta tarefa.","reference":"organizar-notas"}
```
````

`reference` é apenas uma pista de busca por nome ou ID, não caminho ou URL
importável. `skills_brief` fornece nome, descrição e caminho, sem ID explícito:
não adivinhe ID nem presuma uma ferramenta de descoberta ou lista. O texto do
bot não comprova existência, instalação, vínculo, versão ou disponibilidade.

Os exemplos são autorais e sintéticos. O JSON deve ter exatamente os campos
mostrados, com `schema` numérico 1 e strings não vazias. `reason` e `description`
admitem até 1.024 caracteres, `reference` até 256, `name` até 64 e `instructions` até
32 KiB UTF-8. O envelope admite 64 KiB e a mensagem 128 KiB. Deixe margem para
o YAML e o `SKILL.md` final, validados pelo Hub: esses limites não garantem que
o arquivo final caiba. Não inclua autor, bot, destino, grant, aprovação, script,
URL, arquivo, Hub, alvo, versão, instalação ou qualquer outro campo. A origem é o autor real resolvido da
mensagem do chat, nunca um campo emitido pelo modelo; se indisponível, a UI usa
texto neutro.

Ofereça uma única proposta entre os três formatos, fora de citação e de outro
fence. Propostas ou chaves duplicadas, mistura, campos extras, blocos incompletos ou inválidos e
exemplos citados ou aninhados permanecem texto, sem ação. Somente resposta final
real do agente pode produzir cartão; mensagens de usuário, ferramenta, aviso,
trace ou stream não o produzem.

O cartão de criação apresenta autor real, motivo e conteúdo proposto. **Examinar rascunho**
abre o editor existente com campos editáveis. Abrir, cancelar ou **Agora não**
não salva nem liga; a dispensa é apenas local à UI, sem promessa de persistência.
**Salvar** é gesto separado do dono no app pelo `installSkill` manual forte,
protocolo 2; ligar a um bot é outro gesto. Não há ferramenta nova ou efeito do
modelo para propor, instalar, ligar ou abrir o formulário. Sem genUI, novo executor
ou autoridade, grants, scripts, importação ZIP/URL. A proposta e o conteúdo externo
são dados, não pedido ou aprovação do dono; voz não aprova e segredo é só por
referência. As ferramentas de leitura disponíveis e autorizadas continuam
servindo à consulta passiva das referências pertinentes.

O cartão de indicação mostra motivo, **Referência sugerida**, **Examinar
indicação** e **Agora não**. Examinar abre a mesma página Skills em uma folha
somente de leitura, com busca sugerida e abas Instaladas e Catálogo. A pessoa
escolhe um resultado canônico, mesmo quando houver apenas um; nomes iguais podem
ter vários resultados. Os mesmos detalhes são abertos somente para leitura da
versão conferida: ID e digest no catálogo; encarnação, registro, recibo e conteúdo
na instalada. Não invente curadoria ou recibo para texto manual. Instalar e ligar
continuam gestos separados nas telas atuais, ausentes dessa inspeção. Abrir,
navegar, atualizar, fechar ou dispensar apenas consulta; dispensar é local à UI,
sem promessa de persistência. Skill alguma concede ferramentas, conta ou permissão.

A inspeção usa a mesma fonte Pessoal/Hub e sessão capturadas. Origem ou sessão
inválida descarta leitura antiga. Trocar a proposta impede abrir pelo gesto antigo;
a inspeção ou o editor já aberto mantém a cópia escolhida e pode concluir leituras
no mesmo contexto válido. Recomendar existentes em texto continua permitido, sem
exigir cartão.
Erro, offline, protocolo ausente, conteúdo alterado ou indisponível têm motivo:
falha não vira lista vazia, e lista parcial por incompatibilidade não prova
que não há outros resultados. Não repita efeitos automaticamente.

O catálogo é Pessoal: em outro Space não há fallback para Pessoal. Trocar origem
ou produtor invalida a intenção de abertura e preserva o rascunho; recusa ou resultado
incerto não autoriza retry automático. Reconcilie no mesmo destino autorizado.
Não afirme estado de instalação ou ligação sem dados canônicos. Código candidato,
merge, pacote, instalação e prova nativa são estados distintos.

## Formato e descrição que orienta o disparo

Agent Skills usa uma pasta com `SKILL.md`: frontmatter YAML entre `---`, seguido
de instruções Markdown. Exemplo autoral de rascunho:

```markdown
---
name: organizar-notas-de-estudo
description: >-
  Organiza notas fornecidas em tópicos e dúvidas para revisão.
  Use ao pedir um roteiro de estudo a partir dessas notas.
  Não use para buscar fontes, enviar mensagens ou marcar compromissos.
---

Transforme apenas as notas fornecidas em uma sequência de tópicos.
Separe dúvidas de afirmações e preserve as incertezas do material.
Entregue tópicos, dúvidas e uma sugestão de ordem de leitura.
Se faltarem notas, peça o material antes de escrever o roteiro.
```

No formulário manual, preencha os três campos; o Hub gera identificador e YAML.
O exemplo de arquivo não é uma instrução para gravar diretamente no catálogo.

| Elemento | Regra de autoria e compatibilidade |
| --- | --- |
| `name` | Até 64 caracteres, igual à pasta no formato de arquivo. Para novos identificadores próprios, prefira ASCII minúsculo, números e hífens, sem hífen nas pontas ou repetido. O parser local também aceita legados Unicode; este guia não os renomeia |
| `description` | Não vazia, até 1.024 caracteres. Explique o que faz, frases/situações que pedem seu uso e exclusões relevantes; evite descrição genérica, tags e alegações de autoridade |
| Corpo | Objetivo, entradas, passos, saída, exemplo e limites. Mantenha abaixo de 500 linhas como recomendação editorial e dentro de 32 KiB no catálogo; linhas e bytes são limites diferentes |
| Campos opcionais | `license`, `compatibility` (até 500 caracteres), `metadata` com valores textuais e `allowed-tools` textual no parser local. Este último não concede ferramentas; evite-o em conteúdo próprio sem necessidade |
| Referências | Podem ser consultadas sob demanda, com indicação clara de quando abrir. O formulário manual atual gera somente `SKILL.md`; seu texto deve ser autocontido |
| `scripts/` e `assets/` | São possibilidades do formato externo, não do importador atual. Não inclua scripts, executáveis ou assets binários no pacote passivo instalável |

Nome e descrição das skills selecionadas chegam ao perfil do bot; o corpo é lido
quando necessário. `/id` pede leitura de uma skill selecionada como orientação
externa. Só entra a skill cujo texto confere com a instalação no momento da
montagem; alterada fora do app ou ilegível fica de fora, e o `/id` dela avisa
sem caminho. No app, a curada pode ser reinstalada, o que repõe a versão
conferida, ou removida; texto manual não tem restauração: remova e salve de novo
como skill nova. Isso não cobre leitura posterior pelo mesmo caminho. Isso não garante acesso aos arquivos de outras skills em todo perfil: leitura fechada
geral para ambientes restritos continua dependência própria, sem abrir filesystem
livre para contornar a limitação. A exceção documental é somente a projeção
fixa pública de `criador-de-skills` e suas referências junto ao manual do Hub,
legível pela permissão já existente, sem alterar seleção ou outras skills.

## O que existe e o que permanece planejado

O catálogo atual é curado e passivo, restrito a Pessoal, com procedência, digest,
recibo e identidade de instalação. Seus limites são 32 KiB por `SKILL.md`, 1 MiB
por arquivo, 5 MiB/100 arquivos por pacote e profundidade de quatro diretórios.
Aceita Markdown, texto e JSON passivo; recusa links de filesystem, executáveis,
scripts e HTML ativo. Não importa URL arbitrária nem atualiza por branch móvel.

A entrada externa `skill-creator` está indisponível: no pin do catálogo, seu
arquivo tem 33.168 bytes, acima dos 32.768 permitidos. Isso não comprova um
criador integrado. O método pode orientar texto autoral menor; não corte nem
traduza o original silenciosamente para fazê-lo caber. ZIP/`.skill` do ambiente
Claude e o envelope curado do Hub são formatos distintos.

Na criação pelo app, sem escolha explícita, bots começam sem skills; o Berga
inicial também. Criar já com skills exige a instalação vista no formulário
(`createBot` com `skillSelection.installations`). O caminho legado de gestão pelo
Berga ainda copia a seleção do criador (`host/src/chat/manage.rs`), presa à
seleção ORIGINAL vista antes da aprovação, inclusive vazia: se ela mudou até a
gravação, a criação é recusada, sem recapturar. Ligar e desligar seguem a
seleção protegida do [editor](../../features/editar-bot.md); a **Base central**
(Extensões → Skills) mostra a mesma associação `cfg.skills` como tabela Skill ×
Bot do Pessoal e muda uma célula por gesto pelo mesmo produtor. Sim é vínculo, não
leitura, uso, ferramenta ou autorização; instalar continua no Catálogo e não liga. Já o
plano do Criador de bots
exige bots novos sem herança e instalação/ligação por cartão, inclusive em Auto.
Criador de bots e autoria
de skills têm responsabilidades distintas; a implementação futura deve
reconciliar o legado, sem alterar a autoridade por este capítulo.

## Recomendação e revisão

O cartão de recomendação propõe criar uma skill; a indicação aponta uma skill existente
para inspeção somente de leitura. Uma revisão aprovada deverá alcançar todos os bots vinculados;
a operação de edição/publicação global permanece futura. Não se deve simular
edição removendo e recriando a instalação.

### Comparação manual

No app, a comparação prepara um rascunho e compara o documento manual integral
com uma base de revisão comprovada, mostrando digests, revisão seguinte candidata
e bots vinculados. A comparação não salva, aprova, publica ou emite recibo.
Alterar o rascunho exige nova comparação. Não diga que uma revisão foi aplicada.
Curadas permanecem imutáveis; manual antiga sem revisão comprovada conserva a
leitura normal, sem migração que aprove os bytes atuais. Não remova/recrie nem
escreva por shell para simular revisão. Publicação compartilhada aguarda
produtor aceito de leitura nativa/snapshot com revogação. O bot não ganha RPC,
ferramenta ou autoridade; nenhum corpo é anexado a todos os prompts.

Queda ou novo hello no mesmo Hub preservam só o texto/alvo em memória da
abertura, sem base, resultado ou pedido automático. Reler a instalada exige
novo gesto na mesma instalação; outro Hub ou instalação não adotam o rascunho.
Fechar, Voltar, abrir outra linha ou sair da página Skills descarta o texto.

## Marketing passivo

O catálogo curado oferece copywriting adaptada com guia curto autoral no lugar do auxiliar de licença
incerta, stop-slop e copy-review-panel. O dono instala e seleciona em gestos
separados; não há atualização automática de instalações. Marketing-os completo
continua piloto futuro. Os créditos MIT, pins e omissões estão no
[catálogo](../../reference/catalogo-skills.md).
São textos passivos: nenhuma ferramenta, execução, escrita de catálogo ou
autoridade adicional. As regras de conteúdo externo e de aprovação continuam no Hub.

O painel concentra cinco perspectivas numa resposta, cada uma 0–20 (total
0–100). <70: refazer/descartar; 70–84: revisar a principal objeção e conferir
novamente; >=85: candidato para apresentar ao dono. Até três rodadas, sem
bots ou subchamadas pagos e sem publicar automaticamente. A nota é heurística
editorial, sem comprovar conversão, qualidade, segurança ou decisão humana.
Não inventar métricas, depoimentos ou promessas; indicar o contexto realmente
lido e a evidência que falta.

A API candidata de leitura mediada
permite ao consumer ACP obter somente os bytes conferidos de uma skill selecionada
para a mesma origem de turno. Não aceita caminhos. Desligar/reinstalar ou terminar
o turno recusa novas leituras nessa rota, inclusive após espera no transporte.
A candidata não está aceita para publicação, não ativa leitura nativa e não
apaga texto já recebido, copiado ou guardado em contexto/memória. Caminhos legados
e a contenção de pastas continuam dependências separadas.

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
