# Mexerica — vetores criptográficos compartilhados

Documento adaptado em 28/10/2026. [remote-relay-vectors.json](remote-relay-vectors.json) conserva vetores determinísticos do protocolo v1: chaves de Host/dispositivo, handshake, cifra de frames, blocos, SAS, ofertas de pareamento, mailbox, assinaturas, claims, ACL assinada e push selado.

Os valores privados são sementes fixas de teste, não credenciais reais. Os bytes dos vetores não foram alterados; continuam sendo o contrato comum verificado por Rust e TypeScript neste repositório. Um teste de vetor não comprova uma implantação de relay ou disponibilidade do cliente móvel.

[remote-relay-vectors.py](remote-relay-vectors.py) escreve JSON em stdout e depende de Python 3 com `cryptography`. Em verificação autorizada, a partir da raiz, é possível comparar sem sobrescrever a fixture rastreada:

```sh
python3 docs/reference/fixtures/remote-relay-vectors.py > /tmp/donnabot-relay-vectors.json
cmp docs/reference/fixtures/remote-relay-vectors.json /tmp/donnabot-relay-vectors.json
```

[browser-socket-subprotocol.json](browser-socket-subprotocol.json) (MX-199/MX-212) fixa o valor exato que o navegador manda no WebSocket (`codync-sig.v1` + `base64url(utf8(Codync-Sig))`, até 512 bytes decodificados), conferido no desktop contra o `browserSocket` e contra a função de parse do servidor.

Use temporário exclusivo da tarefa quando houver concorrência. Os consumidores existentes incluem:

- `host/src/remote/crypto.rs` (`include_str!`);
- `cloud/test/vectors.test.ts` e `cloud/test/relay.test.ts`.

Ao mover ou alterar uma fixture, confira imports, `kit.yml` e os filtros de caminhos dos workflows, sem ativar Actions por inferência. Rode a validação aplicável no fluxo de AGENTS.md; todo Cargo do Host é exclusivo do servidor. Confira o protocolo. Esta revisão só alterou o README.
