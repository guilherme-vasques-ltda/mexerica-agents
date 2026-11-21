# Armazenamento

Ajustes → Pessoal → Armazenamento mostra um relatório do Hub conectado, medido
quando a página abre e ao clicar em **Atualizar**. O horário é o fim da medição.
A página só consulta. **Não exclui conversas nem libera espaço**;
a ação de limpeza fica para uma atualização futura.

## Quem pode ver

O RPC `donnaStorageReport` usa a mesma autoridade de leitura do editor/WhatsApp:
app local autenticado ou Device com `Control`. Device sem `Control`, pareamento,
bootstrap e transporte de ferramentas de bot não podem consultar. Não há ferramenta
anunciada ao motor; o manual do harness não muda.

## De onde vêm os números

| Número | Fonte e significado |
| --- | --- |
| Banco do Hub | Tamanho lógico do arquivo SQLite principal, por metadados do sistema de arquivos. |
| WAL | Tamanho do arquivo `-wal` do mesmo banco; arquivo ausente vale zero. É somado ao banco no total usado. |
| Livre dentro do banco | `PRAGMA freelist_count × PRAGMA page_size`. Espaço já reservado pelo SQLite que pode ser reutilizado; não é espaço livre no disco. |
| Livre no disco | `fs2::available_space` na pasta do arquivo do banco. Não percorre volumes. |
| Mensagens por bot | Soma de `length(CAST(entries.data AS BLOB))`: bytes UTF-8 de todas as entradas, incluindo ferramentas e avisos. |
| Número de mensagens e desde | Só entradas `kind IN ('user','agent')`; mínimo `created_at`, em milissegundos UTC. Sem mensagens, data ausente. |
| Principal e extras | `session IS NULL` é a principal; `session IS NOT NULL` reúne as extras, inclusive sessões fechadas. |
| Anexos | Soma dos tamanhos de arquivos regulares sob `chat::uploads::root(cfg)`, recursivamente. Inclui anexos ainda presentes mesmo sem referência em mensagem. |
| Memória, linhas | Conteúdo textual UTF-8 atual e das versões (`memory_entries`, `donna_memory_versions`), incluindo texto, evidências, origem e texto de conflito; conteúdo, origem e conflito de `memory_file_versions`; nome, hash e descrição de `memory_files`. Filtrado pelo ID do bot. |
| Memória, arquivos | Arquivos regulares na pasta canônica `bot-memory` do bot, sem criar pasta ou escanear/gravar versões. O arquivo atual e as versões no SQLite são cópias persistidas distintas e entram separadamente. |
| WhatsApp, caixa do Hub | Bytes UTF-8 de corpo, mídia serializada, nome da conversa, nome e telefone do remetente em `donna_whatsapp_messages`, por bot. Não executa expiração nem marca leitura. |
| WhatsApp, wacli | Apenas metadados dos arquivos `whatsapp/<bot>/wacli.db` e `wacli.db-wal`; nunca abre esses bancos. Ausência dos arquivos vale zero. |
| Total por bot | Soma dos conteúdos de mensagens, anexos, memória e WhatsApp. Só existe se todas essas partes estiverem disponíveis. |

A lista inclui apenas bots vivos (`bots.deleted=0`) do Space solicitado, com seus nomes. O total do banco e do disco é contexto global do Hub (`total.context = "global"`), igual para qualquer Space. Sem `spaceId`, o Hub usa o Pessoal. O app ordena
pelo conteúdo medido, do maior para o menor. O tamanho físico do banco **não é
rateado** entre bots: o total por bot é conteúdo lógico, e não uma estimativa do
espaço que uma exclusão devolveria ao disco.

## Limites e falhas

A consulta roda em `spawn_blocking`, fora do ator. A conexão compartilhada do Hub
é usada somente para descobrir o caminho do banco; o restante usa conexão SQLite
`READ_ONLY`, sem `Store::open`, migração, transação de escrita ou checkpoint.
São consultas separadas por bot e componente, sem transação longa. O relatório é
uma medição aproximada no tempo: conversas e arquivos podem mudar durante a leitura.

Parte que falha volta como `null` e aparece como **Indisponível**. O subtotal que
depende dela também fica indisponível. As outras partes continuam. Pasta/arquivo
opcional comprovadamente ausente vale zero; erro de acesso não vale zero. Links
simbólicos e tipos especiais encontrados nas pastas não são seguidos: tornam a
parte indisponível. Falha ao listar bots devolve lista indisponível, preservando os
totais que foram medidos. Logs ficam no Hub; erros internos não viram texto da tela.

Não são medidos por bot: índices/FTS, cabeçalhos e metadados estruturais do SQLite,
memória compartilhada, tabelas auxiliares de sonhos/tombstones/bloqueios, outros
arquivos do workspace, backups, `session.db` do WhatsApp, mídia externa e
transcrições internas dos motores Claude/Codex. Índices e tabelas no banco do Hub
já fazem parte de seu tamanho físico total. Os diretórios de transcrições dos
motores existem, mas a atribuição por bot depende do motor e da sessão: ficam para
uma atualização futura, sem percorrer dados globais de contas.

WhatsApp mantém sua própria política de guarda: a página informa isso em texto,
sem link que leve a uma opção inexistente, e a limpeza não inclui o WhatsApp.

## Compatibilidade e idiomas

Hub sem o método mostra **Atualize o Hub para ver o armazenamento**. O relatório
é resposta pontual de RPC; não altera bot, entrada ou hello e não é persistido no
cache espelhado. Portanto **não muda `MIRROR_SCHEMA`**.

Textos em português, sem stub de `t` nem catálogo paralelo. O namespace reservado
é **`armazenamento.*`**.

## Disponibilidade por Space

A capacidade `spaceStorageContract` ainda não é anunciada. Hubs antigos
retornam bots de todos os Spaces, portanto não devem ser usados para essa tela por Space.
