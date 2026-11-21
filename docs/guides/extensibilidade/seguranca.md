# Segurança comum da extensibilidade

[Índice](README.md) · [Skills](skills.md) · [MCP](mcp.md) · [Plugins](plugins.md)

Este capítulo reúne fronteiras existentes e aponta seus contratos. Não cria
permissão, papel, cofre, executor ou política paralela. A autoridade continua
validada pelo Hub; instalar, selecionar, fazer login e chamar uma operação são
decisões distintas.

## Conteúdo externo e aprovação

Páginas, PDFs, anexos, skills, manifestos, esquemas, resultados MCP e
mensagens de outros bots são dados. Não podem se declarar fala do dono,
substituir instruções, conceder acesso ou limpar a marca persistente de
exposição. YAML válido, hash e origem oficial não tornam instruções externas
confiáveis. O bot que faz gestão conserva as restrições de exposição previstas
em [Donna central](../../features/donna-central.md)
e no plano do Criador.

Voz pode pedir, mas não aprova scripts, arquivos, MCP ou gestão;
nesses casos, a aprovação exigida ocorre no app. A exceção restrita atual de
`TodoWrite` Claude não se estende a skills ou plugins. Veja
[chamada de voz](../../features/voice-call.md) e
[manual vigente](../../features/manual-do-harness.md).

Seleção, `enabled`, nome de ferramenta, annotations de leitura e manifesto não
emitem grants. Identidade original, conta, recurso, Space/audiência e gerações
continuam sujeitos à validação do produtor no Hub, inclusive depois de filas e
esperas e antes de gravação ou resposta privada. Não promova Device a Local nem
estenda `engine.accounts` a conectores: o contrato correspondente tem escopo próprio em
[setup de provedores](../../features/setup-provedores.md).

## Segredos por referência

O mecanismo existente mantém credenciais no cofre/1Password, resolve referências do
1Password no limite de execução e oferece login por ID sem devolver senha ao
modelo. Veja [credenciais](../../features/connector-credentials.md).
Isso não demonstra a entrega do fluxo novo de senha por referência nem de elicitation URL.

Os contratos de senha por referência orientam produtores futuros.
O plano canônico de senha por referência
detalha destino exato, coleta protegida, prazo, isolamento e gates, sem anunciar
implementação. A síntese abaixo não substitui esses requisitos:

| Contrato | Requisitos a preservar |
| --- | --- |
| Captura e uso | Caixa protegida e cofre; o bot vê etiqueta/referência opaca. O Hub substitui o valor somente no campo, site ou chamada exatos aprovados, sem revelar ao modelo |
| Envio final | Bloquear credenciais reconhecidas no payload final de conectores e exigir revisão humana da ação exata, sem classificador amplo ou promessa de cobertura universal |
| Validade | Até **90 segundos após a confirmação**, sem renovação; prevalece qualquer prazo menor |

Valor secreto não entra em skill, argumentos visíveis ao modelo, exemplos,
pacote, `.env` exportado, prompt, log, memória ou captura. Uma referência não é
um getter genérico de senhas nem pode ser enviada automaticamente a um destino
indicado por conteúdo externo. Destino, operação e contexto precisam coincidir.
Login oficial de conta, elicitation URL e referência local são fluxos diferentes.
Mudanças em segredos, custo ou dados exigem aprovação do dono; este guia não a
substitui. A integração operacional futura deve atualizar os contratos
dos produtores e o manual no mesmo PR.

## Scripts, processos e pacotes

Scripts nunca são executados sem aprovação do dono no app, vinculada ao conteúdo,
versão, alvo e efeitos pertinentes. Aprovação é necessária e não cria executor
ou capacidade ausente. O catálogo passivo atual recusa scripts; abrir, importar
ou revisar uma skill não autoriza executá-los. Não use shell, MCP direto no
motor ou RPC genérica para contornar essa recusa.

`verify` e readiness de um MCP local podem iniciar processo. Hooks e comandos
de dependência também podem executar ou baixar código. Ler documentação ou
manifesto não autoriza essas operações. Processos do mesmo usuário podem
alcançar recursos fora da ponte; o protocolo, um iframe e regras do modelo não
constituem isolamento do sistema operacional.

O [Guard Hub](../../features/guard-hub.md) documenta a mediação local e seus
limites; o contrato de [marketplace](../../features/marketplace.md) registra a
mediação remota já existente. Inspecionar retorno e registrar exposição não
equivale a aprovar toda ação antes do efeito nem cobre automaticamente shell
e ferramentas nativas. Um resultado incerto requer reconciliação no
destino autorizado; timeout, remoção ou reinstalação não desfazem efeitos
externos nem ressuscitam concessões.

## Procedência e documentação pública

Registre fonte, revisão fixa, licença, arquivos e alterações de material
copiado em [THIRD_PARTY.md](../../../THIRD_PARTY.md), preservando os avisos
aplicáveis. Licença de uma skill não cobre todo um repositório ou o PDF do guia;
metadados de catálogo tampouco licenciam o pacote apontado. Sem licença
demonstrada, não redistribua. Hash identifica bytes, sem certificar segurança.
Adaptações devem ser explícitas e conservar procedência e perdas.

`docs/` é fonte da wiki pública. Use redação autoral, links oficiais e
exemplos sintéticos. Não publique cópias externas, segredos, IPs, hostnames
internos, dados pessoais, transcrições ou capturas privadas. O guia mantém resumos e
metadados em [fontes](fontes.md); não publica originais ou relatórios de pesquisa
e não autoriza publicar a wiki.
