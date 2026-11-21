// Só na demo: `donnaRoutingReceipt` sem recibo de roteamento e com o uso do turno que o motor informou.
// Uma contagem fica sem número de propósito (cache gravado): o recibo mostra "não informado", nunca zero.

export class FakeRecibo {
  constructor(private readonly hubId: () => string) {}

  atende = (method: string) => method === 'donnaRoutingReceipt'

  async call(): Promise<unknown> {
    return { hubId: this.hubId(), state: 'notRouted', receipt: null, usage: { input: 18_420, cacheRead: 61_210, cacheWrite: null, output: 932 } }
  }
}
