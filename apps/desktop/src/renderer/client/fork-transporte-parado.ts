import { HostError, type HostTransport } from './host-client'

/**
 * MX-217 (store/fork-queda-curta.ts): a falha de um link caído ou de uma geração antiga do link. Status 0 como
 * "inalcançável", mas não transitória: `withLink` não repete a operação em silêncio por outro link; o envio fica
 * "não enviado", com Tentar de novo.
 */
export class LinkCaido extends HostError {
  constructor() { super(0, HostError.unreachable().message) }
  override get isTransient() { return false }
}

export const linkCaido = (): HostError => new LinkCaido()

/** O transporte de um link caído: falha na hora, sem rede; nunca reusa a porta ou o canal antigos. */
export const transporteParado: HostTransport = {
  call: () => Promise.reject(linkCaido()),
  stream: (_path, _onData, onEnd) => {
    let ativo = true
    queueMicrotask(() => { if (ativo) onEnd(linkCaido()) })
    return () => { ativo = false }
  },
}
