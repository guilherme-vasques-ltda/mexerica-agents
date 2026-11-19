# Mexeriquinhas — Simples e Pelúcia 3D

<!-- SPDX-License-Identifier: Apache-2.0 -->

Biblioteca original aprovada pelo dono no [PR #600](https://github.com/example-org/mexerica/pull/600), mantido como referência em rascunho. A fase 2a integra os dois estilos ao desktop; gerador por IA é fase 2b e limpeza dos robôs é fase 2c. Os mesmos 119 nomes em 17 categorias D526 escolhem só aparência, sem criar instruções ou permissões.

## API visual

```ts
import { modeloPorId, conjuntoDoModelo, renderSimples, renderPelucia, resolveState } from './index.ts';
const modelo = modeloPorId('pomarca')!;
const state = resolveState({ ocupado: true }, Date.now());
const svg = renderSimples(modelo.config, { size: 32, state });
const html = renderPelucia(conjuntoDoModelo(modelo), {
  size: 32, state, assetRoot: '/assets/', label: modelo.nome,
});
```

`renderAvatar` e `renderSimples` são a mesma API SVG original. `render-simples.ts` e o CSS vetorial foram preservados da aprovação; IDs únicos, textos escapados e simplificação até 40 px. O app usa um adaptador React próprio para visibilidade e dados persistidos, sem alterar os desenhos.

Simples permite Olhos, Cor, Roupa, Acessório e Objeto. “Crie o seu” começa em `CONFIG_BASE`, sem chave. O editor copia os dados e não modifica `MODELOS`.

Pelúcia tem 21 conjuntos prontos (15 bases, Berga e 5 diretores) mais a base neutra original. Os 119 nomes compartilham essas aparências. Cada conjunto contém 9 quadros WebP transparentes em 64, 128 e 256 px (degrau pelo tamanho de exibição): Livre 1/2, Piscar, Trabalhando 1/2, Precisa de você, Erro e Dormindo 1/2. `renderPelucia` devolve HTML; o adaptador do app resolve URLs emitidas pelo Vite e monta só os quadros visíveis do estado atual. Miniaturas paradas usam 1 quadro.

`EscolhaAvatar` é um tipo da prévia da fase 1 (`simples` ou `pelucia`). Ele não é o contrato persistido do Hub. No app, `shared/fork-mexeriquinha-config.ts` define v2 (`simples` ou `3d`), validação, projeção legada e fronteiras. `avatarConfig` público continua legado; a nova aparência fica na chave autorizada `mexeriquinha`. Não persista `baseUrl` ou HTML retornado por esta biblioteca.

## Estados e movimento

`resolveState(sinais, now)`: erro > precisa-resposta > trabalhando > dormindo > livre. Dormir exige mais de 8 h estritas; exatamente 8 h, ausência de registro ou horário futuro dão Livre. `mexeriquinhas-runtime.ts` liga esses sinais ao status real do bot e preserva Estado indisponível.

O CSS usa `mxq-*` para SVG e `mxp-*` para Pelúcia. Pausa por ancestral `mxq-pausado`/`mxp-pausado`; ambos respeitam `prefers-reduced-motion`. O app pausa também fora da vista e com a janela oculta/coberta.

Na aprovação, o bundle visual completo (JS/CSS/catálogo) media 44.426 bytes minificados /12.482 gzip. SVG: 31–45 elementos em 24/32 px e 38–54 em 96/128 px. Pelúcia: 1–3 imagens e 0 SVG. Os 396 WebP somam 3.941.930 bytes; são arquivos separados. O peso da integração no app precisa ser medido no build da fase 2a, não inferido desses números da biblioteca.

## Origem e escopo

Código, prompts e ilustrações originais para Apache 2.0. A base PNG é nossa referência de geração; não é raster de um avatar em uso. Os prompts versionados e metadados de geração são procedência; não fazem chamadas de API no app.

A prévia e o servidor de geração da fase 1 permanecem no PR #600. Este diretório não inclui o protótipo de servidor nem usa chave de IA. O gerador integrado ao Hub/cofre depende da fase 2b e do SIM da Segurança.

Contrato e evidências da integração: fase 2a.
