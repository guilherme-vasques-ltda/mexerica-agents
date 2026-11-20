# Origem do helper de fala

`Package.swift` e `Sources/main.swift` foram restaurados em 19/11/2026 da
árvore fonte da Mexerica, commit
`d6a75dd0afb0bcddc917bcebcf7c8701085fc0f9`, nos mesmos caminhos. A árvore fonte
adota a licença [Apache-2.0](../../../../LICENSE) e mantém as atribuições ao
Codync em [NOTICE](../../../../NOTICE). O pacote não declara dependências de
terceiros nem inclui código de frameworks: usa apenas o SDK da Apple.

Nesta cópia, o helper foi alterado para recusar a captura se o idioma não
oferecer reconhecimento local e para impedir que um pedido aceito envie
áudio pela rede. A lista de idiomas inclui só os que oferecem esse suporte.
