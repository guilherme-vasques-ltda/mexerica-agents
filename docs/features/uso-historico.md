# Histórico de uso da assinatura por provedor

O Hub guarda amostras do uso da assinatura **por provedor** (`claude`, `codex` — o login é global, nunca por conta) para mostrar o histórico recente e estimar quando a janela pode acabar. Só o Hub grava e calcula; o cliente só renderiza.

## O que é registrado

A tabela própria `donna_usage_samples(provider, window_id, slot, at, percent, resets_at)` recebe no máximo **uma amostra por vaga de 10 minutos** por par provedor/janela — o corte vale para todas as fontes (a barra de status do Claude chama várias vezes por minuto): primeiro em memória, garantido também no banco. Janela sem percentual não grava.

`slot` marca o instante que **abriu** a vaga (a chave de dedupe); `at` marca o instante da **observação que definiu o ponto atual**. Uma leitura dentro da vaga **atualiza a mesma linha**: `at` preserva o primeiro 100% enquanto a nova leitura também for ≥ 100 e a janela continuar vigente; queda, troca de reset ou reset vencido fazem `at` avançar para a nova observação. O percentual é sempre o mais novo e `resets_at` é preenchido quando a leitura o informa — a CLI do Claude não informa reset e a statusline informa, então as duas podem alternar sem apagar o reset conhecido vigente nem criar amostra extra; um reset vencido não é herdado.

Amostras com mais de **31 dias** são apagadas junto da gravação, no máximo uma varredura por hora e em lotes limitados. Se a gravação falhar, a leitura de uso segue normal (aviso técnico no Hub, sem dado sensível) e nenhum marcador fica preso: o cache só é atualizado depois do commit.

## Consulta

`donnaUsageHistory {range}` é RPC privado e somente leitura do dono (local ou aparelho com Control). A autoridade original é revalidada na transação do ponto de gravação do dono, que só captura a **identidade do arquivo** do banco (dev+inode); a consulta roda numa conexão própria somente leitura, **fora da transação de autorização e do mutex**, em uma transação de leitura com snapshot consistente, depois de reconfirmar dev+inode do caminho e o handle aberto — arquivo trocado entre as etapas falha fechado, com custo limitado: os blocos vêm agregados do próprio SQL e cada janela lê só suas amostras recentes via índice. `range` aceita `"24h"`, `"7d"` ou `"30d"`; qualquer outro valor devolve `invalid_usage_history_range`. A resposta traz `range`, `bucketMs`, `measuredAt` (UTC ms), `truncated` e `windows`, cada uma com `provider`, `windowId`, `resetsAt` vigente, `buckets` e `forecast`. Limites defensivos: até **8 janelas** (Claude e Codex 5h/7d dão ~4; o resto é folga), **1024 blocos** por janela e **300 amostras** recentes por janela (~50 h de cobertura) — corte de janelas ou blocos vira `truncated: true`, nunca silencioso, e a janela afetada recebe `forecast: null`. O recorte das amostras recentes não é corte real e não marca `truncated`: se as amostras lidas são todas da mesma janela vigente, a previsão sai normal; se o que se vê não permite concluir nada e a vigência pode continuar fora de vista, a janela recebe `forecast: null`. Custo medido no teste de volume (SQLite em memória, ~35,7 mil amostras sintéticas = 8 janelas × 31 dias): ver `limpeza_e_relatorio_usam_indice_e_o_custo_e_limitado` — como a leitura roda numa conexão própria somente leitura, nenhum desses tempos segura o mutex do banco; o que fica dentro da transação autorizada é só o `stat` da identidade.

Os blocos já vêm agrupados: **24h** em blocos de 10 minutos, **7d** por hora, **30d** em blocos de 6 horas — eixo UTC em ms (`start`), contado pela observação `at`. O valor do bloco é o percentual da **última amostra** daquele bloco; blocos sem amostra não aparecem.

## Previsão

`forecast` estima quando a janela chega a 100%, só a partir da **janela vigente**: o sufixo cronológico que sempre inclui a última amostra, com resets conhecidos compatíveis e percentual sem queda. Reset desconhecido não marca troca; valores conhecidos diferentes, reset vencido antes de uma nova leitura ou queda de percentual marcam reinício. O primeiro 100% é procurado no histórico retido, também fora do range exibido.

- `exhausted: true` — a janela **está esgotada como fato observado**: o último percentual visto já é ≥ 100. `exhaustsAt` é o instante da primeira amostra ≥ 100 da vigente — nunca no futuro e nunca concluído da reta.
- `exhausted: false` com `exhaustsAt` — a reta vigente é consistente com a última leitura e cruza 100% depois de agora e antes de `resetsAt`.
- `exhausted: false` com `exhaustsAt: null` — sem dado suficiente (<4 amostras ou <30 min de cobertura), sem tendência de alta, reta que não explica a última observação (erro > 2 pontos), `resetsAt` desconhecido, `resetsAt` vigente já passado (a janela rolou e um 100% antigo não vale para a nova), ou cruzamento depois do reset.
- `forecast: null` — o corte de janelas/blocos ou uma incerteza real no recorte das amostras impede qualquer conclusão; nunca é silencioso (vem acompanhado de `truncated` ou do recorte documentado acima). `exhausted: true` só sai de fato observado e sobrevive a qualquer corte.

A fórmula é regressão linear simples `percent = a + b·at` sobre as amostras vigentes, aceita somente quando o erro na amostra mais recente é ≤ 2 pontos percentuais. A estimativa é conservadora e não substitui o painel do provedor.

`MIRROR_SCHEMA` não muda: o histórico só sai por RPC, sem campo novo no bot, hello ou entradas espelhadas.
