# Tabela de preços de API (`precos.json`)

Tabela versionada de preços de API usada para **estimar** o custo de um turno
quando o motor não informa valor nenhum (ex.: assinatura). Estimativa por
tokens × preço de API; nunca muda o histórico e nunca se soma ao custo
informado sem distinção — o relatório separa `costs` (informado, `source`
`motor`) de `estimatedCosts` (`source` `estimado`).

## Como atualizar

1. Abra a página **oficial** de preços do fornecedor e leia os números lá.
   Regra dura: só entra linha (e campo) cuja URL oficial você abriu e cujo
   número leu. Sem fonte verificada, o modelo fica **sem preço** — não
   invente número.
2. Edite `precos.json`: chave = ID do modelo exatamente como o motor o
   relata; `input`, `output`, `cache_read`, `cache_write` em **USD por 1
   milhão de tokens** (`cache_read`/`cache_write` só entram se lidos na
   página); `fonte` = URL oficial; `data` = `AAAA-MM-DD` da consulta.
3. Modelo com preço progressivo por tamanho de prompt usa `"faixas"`: uma
   lista ordenada, cada item com `ate` (tamanho máximo do prompt, em tokens,
   coberto pela faixa — `input` + `cache_read` + `cache_write` do turno) e os
   preços daquela faixa; a última faixa não tem `ate` e cobre qualquer
   prompt maior. Só entra faixa lida na página oficial; prompt fora de toda
   faixa verificada fica **sem preço**.
4. Aliases (`"aliases"`) só quando a própria página oficial declara a
   equivalência (ex.: nomes legados que a página diz que seguem o mesmo
   preço). Casamento do modelo é **exato**: sem prefixo frouxo nem sufixo
   adivinhado. Variante de data/contexto que a página não declara vira linha
   própria ou fica sem preço.
5. Rode os testes de `fork::custo`: há um teste que desserializa o arquivo
   com o mesmo tipo tipado da produção (preços finitos, não negativos, com
   até 6 casas decimais; `fonte` e `data` presentes; faixas ordenadas com
   `ate` crescente; alias apontando para modelo existente). Texto inválido
   falha o teste e, em produção, deixaria tudo **sem preço** (sem pânico).

## Regras de cache na estimativa

- Se a faixa aplicável tem `cache_read`/`cache_write`, o turno precisa trazer
  a contagem correspondente — salvo a exceção medida por adaptador abaixo.
  `0` explícito vale zero.
- Contagem **ausente sem prova** de omissão pelo adaptador = **sem preço**
  sempre, mesmo quando a linha não tem preço de cache: não há evidência de
  que o cache era zero.
- Se o turno reporta tokens de cache de um tipo sem preço verificado na
  faixa, o turno fica **sem preço** (não se desconta nem se adivinha).
- Campo de uso **presente mas não representável** (não inteiro, negativo ou
  acima de `i64::MAX`) não é ausência: o turno fica **sem preço** sempre,
  mesmo quando a faixa não tem preço de cache.

## Cache ausente por adaptador (`politicaCache`)

Adaptadores medidos (thread thr_p2e28pdris, só leitura; `fonte` = arquivo e
linha) diferem no que fazem com a contagem de cache quando ela é zero. A
regra só vale para a **versão pinada** (`agentInfo.version` do handshake):
versão diferente, backend sem entrada na tabela ou campo marcado `"sem
prova"` → ausência = **sem preço** (fail-closed), **inclusive quando a
linha não tem preço de cache**. O ponto de decisão é
`precos::politica_cache` (aplicado em `cache()` dentro de `estimar`).

| backend | adaptador/versão | `cachedReadTokens` | `cachedWriteTokens` |
|---|---|---|---|
| claude | claude-agent-acp 0.106.0 | sempre emite → ausente = **sem preço** | sempre emite → ausente = **sem preço** |
| codex | codex-acp 2.6.0 | sempre emite → ausente = **sem preço** | sem prova → **sem preço** |
| gemini | gemini-cli 0.68.0 | omite quando 0 → ausente = **zero** | nunca emite → **sem preço** (ausência não prova escrita zero) |
| opencode | opencode acp 2.2.7 | omite quando 0 → ausente = **zero** | sem prova → **sem preço** |
| grok | @xai-official/grok 1.0.82 | sem prova → **sem preço** | sem prova → **sem preço** |

minimax-code e qwen-code não devolvem uso algum (nem entrada/saída), então já
caem em **sem preço** independentemente de cache.

## Convenções adotadas nesta tabela

- Plano **padrão** de pagamento por uso (standard/pay-as-you-go): não usamos
  faixas Batch/Flex/Priority nem preço promocional de região alternativa.
- **Faixas por tamanho de prompt** modeladas no próprio arquivo
  (`"faixas"`), com os limites lidos nas páginas oficiais: OpenAI ≤272K,
  Grok <200K, Gemini Pro ≤200K, Qwen Coder 32K/128K/256K/1M, MiniMax M3
  ≤512K, Claude Haiku 5.5 ≤100K. A última faixa só fica sem teto quando a
  página a descreve como "> X" sem limite (OpenAI, Grok, Gemini, MiniMax,
  Haiku); Qwen Coder publica só até 1M → prompt acima de 1M fica **sem
  preço**. O uso do ACP é o **agregado do turno** (pode somar várias
  chamadas ao modelo): a faixa é escolhida pelo agregado (input + cache
  lido + cache gravado do turno), não pelo tamanho real de cada chamada —
  várias chamadas curtas podem cair na faixa cara, o que **superestima** e
  é conservador para o teto.
- **Cache de escrita** usa a retenção **assumida por linha** quando a página
  distingue duas tarifas: linhas Anthropic usam a escrita de **5 min** (a de
  1h custa mais — **subestima** sessões que seguram o cache por mais tempo);
  linhas OpenAI usam a coluna de **12h** (a de 30 min custa menos —
  **superestima**, conservador para o teto). Em ambos os casos é estimativa
  para o painel, não teto exato. As demais linhas não têm `cache_write`
  porque a página lida não traz preço separado (a escrita entra como
  entrada normal no provedor).
- **Cache de leitura**: preços explícitos onde a página lista (Anthropic,
  OpenAI, Gemini "context caching", xAI, DeepSeek cache-hit, MiniMax, Z.AI).
- **DeepSeek**: usamos a faixa **peak** (maior); off-peak pode sair mais
  barato que o estimado.
- **Qwen**: valores da região International (Singapura).
- Modelos gratuitos constam com preço `0` — estimativa legítima de US$ 0.
- Modelos de assinatura sem preço por token público (Copilot, Cursor…) e
  páginas cujo número não foi possível ler na fonte oficial (ex.: tabela da
  Kimi renderizada por script) ficam **sem preço** de propósito.
