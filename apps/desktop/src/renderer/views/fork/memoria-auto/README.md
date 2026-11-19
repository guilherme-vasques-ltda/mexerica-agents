# Memória automática — UI Parte A (D246)

Contrato: `host/src/fork/memory_auto/CONTRATO.md`. A View apresenta dados do Hub,
nunca deriva autoria do autor que gravou nem restaura texto/origem por conta própria.
O aviso é uma linha persistente após a resposta. Legado pendente não tem pergunta
ou aceitar/recusar. Notas usam o aviso genérico sem expor seu conteúdo completo.

`MemoryUndoState` conserva a incerteza de transporte por recibo e só confirma
`undone`/`changed` retornados pelo Hub ou recebidos no stream. Repetir `undoMemory`
com o mesmo `entryId` reconcilia o resultado idempotente. Uma revisão posterior
prevalece. `previousOrigin` é dado do Hub; o renderer não a aplica sozinho.

`presentation.test.ts` cobre origem sem prova/mista, pendências sem falso sucesso,
fila cheia/expirada, timeout, repetição do recibo, resposta de outro recibo,
duplo clique e edição posterior. Testes e typecheck são centralizados pelo root;
não foram executados pelo dev UI nesta etapa.

## Fixture opt-in da demo

Com dependências já presentes, iniciar somente Vite com `vite.demo.config.ts`.
No browser-harness, importar manualmente `demo.ts` pelo Vite e chamar
`installMemoryFixture(window.donnabotEditorDemo.host)`. Retorna `requests`,
`setUndoMode`, `addNotice`, `view` e `restore`. Não é importada pelo renderer nem
pelo preload. Serve dados fictícios; não prova Hub, executor, cota, autoridade,
app instalado ou uso de modelos reais.

Prova autoral no Chromium da sandbox: salvo com Dito por você / De fora e-mail;
timeout desconhecido; duas chamadas ao mesmo ID confirmando Desfeito; mudou depois;
sem seleção, capacidade temporária, cheia, expirada e legado pendente sem pergunta;
listagem com as quatro classes de origem. Nenhuma etapa B, Mac, build, instalação,
publicação ou mutação Git foi executada pelo dev UI.

O recibo também aceita `messageIds` humanos imutáveis e `responseIds` do checkpoint
bem-sucedido, fornecidos pelo Hub para vínculo explícito. O renderer preserva esses
campos; não deduz sucesso nem autoridade a partir deles. Agrupamento atual por
turno permanece, sem tocar partes alheias do chat.

`placement.ts` ancora só avisos de memória em `responseIds` explícitos de uma
resposta carregada no mesmo chat/turno. Campo explícito vazio ou ID inválido não
usa fallback. Legado só usa o último agente final comprovadamente do mesmo
chat/turno. Sem alvo, conserva a posição independente com rótulo neutro. O gancho
em `chat-items.ts` não altera a ordem dos demais itens nem dos passos de ferramentas.
`placement.test.ts` cobre recibo atrasado depois do pedido seguinte, IDs ausentes,
alheios/de outro turno/humanos, fallback legado restrito e `stepsByReply` invariável.

Notas legadas com disposição explícita (`jobId`, `availability` ou status da
migração) aparecem individualmente como diagnóstico genérico. `migratedMemoryNote`
nunca transporta texto/conteúdo, confirmação nem revisão para undo. Sem disposição,
`memoryFile` mantém a ocultação legada. O pequeno gancho de `ChatRows` encaminha
apenas esse diagnóstico ao `MemoryNotice`, sem exibir o `data.text` antigo.

Natureza é independente de origem: `said|inferred|conflict|unknown`, classificada
somente pelo Hub. O selo usa natureBot real para identificar quem inferiu, sempre
como hipótese. Conflito mostra texto anterior preservado e alternativa, sem eleger
vencedor. previousVersion/previous* são o snapshot recebido, inclusive data 0 como
Data não registrada. O undo apenas apresenta o estado confirmado do Hub; não
reclassifica texto ou restaura metadados no cliente. O aviso de nota segue sem
expor corpos completos; as versões ficam acessíveis na tela de memória.

Contrato final do undo: o bloco memory no aviso é o recibo histórico imutável da
alteração desfeita. Mudam status e a revisão externa do Entry; metadados dentro do
bloco continuam sendo a versão desfeita. O aviso a rotula Versão desfeita, sem
copiar previous* para a versão atual. A lista consulta o Hub para o estado restaurado.

## D259 P0 — Não salvei

A aba de memória lê somente `memory.nonSaved` do endpoint/contexto existente.
São até 50 ocorrências passadas já filtradas pelo Hub, sem paginação, contagem,
corpo, hash, alvo de duplicata ou indicação de outro escopo. `automatic` conserva
o estado atual. “Foi adiado” não promete uma tentativa futura nem uma fila atual.
Não há aceitar, desfazer ou reenviar neste histórico.

`non-saved.ts` valida os oito motivos e as combinações fechadas de status e
availability confirmadas em CONTRATO.md; enums/linhas incompatíveis são omitidos,
sem fabricar causas a partir de automatic ou recibos. wrongBot só é apresentado
se vier do Hub; o produtor confiável é reservado à futura B. A projeção visual
retém apenas identificador interno, motivo, status e data; não exibe IDs/referências.
Data zero legada aparece como Data não registrada. A ordem é a entregue pelo Hub.

Campo ausente significa histórico indisponível. Erro/timeout da consulta significa
resultado desconhecido; não lista vazia. A cada load/refetch por memoryRev o estado
da consulta invalida a apresentação anterior. Respostas antigas continuam sob a
cerca request existente do MemoryStore. Erros de mutação não reclassificam o
histórico. O painel Bot de fundo/#178 e seus ganchos aprovados ficam intocados.

`non-saved.test.ts` cobre os oito motivos, ausente/vazio/inválido, enums futuros e
combinações incompatíveis, projeção sem campos livres/refs, data sem prova e limite.
Os runners continuam centralizados pelo coordenador. A fixture opt-in oferece
`setNonSaved` e `setMemoryFailure`; somente metadados sintéticos, sem provar Hub,
escopo real, executor ou Parte B. D263/104A recall, P1 e P2 ficam fora desta entrega.

Fronteira do legado: o DTO real de `Origin::default` é
`{schema:1,kind:"unknown",sources:[],external:true}`. A flag é conservadora, não
prova de uma fonte externa. O rótulo continua Origem não registrada; o componente
preserva a flag e a classe/estilo external, sem inventar procedência ou limpar
restrições. `presentation.test.ts` cobre esse DTO exato sem mutação dos dados.
