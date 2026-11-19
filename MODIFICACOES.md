# Modificações desta cópia

Esta cópia deriva de um projeto maior (Apache-2.0, baseado no Codync). Em atendimento à Apache-2.0 §4(b), os arquivos foram **modificados** e este aviso registra as mudanças de forma geral:

1. **Seleção:** só os componentes listados no [README](README.md) foram mantidos; o restante do projeto de origem não faz parte desta cópia.
2. **Configuração neutra:** `apps/desktop/config/*.plist` sem nuvem e sem conta; endereços e domínios de operação substituídos por domínios reservados (`*.example.invalid`); referências a repositórios privados do projeto de origem substituídas por `example-org` ou removidas.
3. **Arte e marcas:** logos de agentes de terceiros e o logotipo da Apple removidos (o app usa ícones genéricos no lugar); as logos dos serviços do catálogo de conectores foram mantidas, só para identificar cada serviço (ver `THIRD_PARTY.md` e `TRADEMARK.md`); mascotes, ícones, avatares e fundos do projeto de origem substituídos por imagens neutras geradas por código, mantendo nomes de arquivo e dimensões.
4. **Testes:** os testes que dependem da arte original, do código de nuvem ou do índice interno de segurança foram marcados com `skip` e o motivo; nenhum teste foi renomeado ou apagado.
5. **Documentação:** README e SECURITY reescritos; trechos do guia do Mac que descreviam uma nuvem de produção foram trocados por uma nota neutra; links para documentos que não fazem parte desta cópia foram removidos (o texto do link foi mantido).
6. **Avisos:** `LICENSE`, `NOTICE` e `TRADEMARK.md` não foram alterados. `THIRD_PARTY.md` é herdado do projeto de origem e pode citar material que **não** está nesta cópia; a seção sobre SF Symbols foi acrescentada e a das marcas de serviços descreve as logos incluídas. O texto herdado aguarda revisão jurídica para ser reduzido ao que de fato é distribuído.

7. **Aviso em cada arquivo modificado:** os arquivos que existem no Codync v2.20.3 e foram alterados nesta cópia trazem, na primeira linha (depois de uma linha `#!`, quando houver), o aviso `Modified from Codync v2.20.3 …`. A lista completa, inclusive dos arquivos que não aceitam comentário (JSON, Markdown, imagens, bloqueio de dependências), está em [MODIFICACOES-ARQUIVOS.md](MODIFICACOES-ARQUIVOS.md). A comparação foi feita com a tag v2.20.3 do Codync (commit `c7548b2896ba40192973760015be9d220dab95e7`); arquivos renomeados e modificados ao mesmo tempo constam só desta descrição geral.

Os identificadores técnicos herdados (por exemplo `app.donnabot.mac`, nomes de binários `donnabot-*` e a pasta de dados) foram mantidos por compatibilidade e não indicam parceria nem origem comercial.
