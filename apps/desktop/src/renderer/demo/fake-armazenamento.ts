// DON-905 (C6), só na demo: `donnaStorageReport` com dados fictícios, no formato do contrato do Hub.
// `?armazenamento=completo|parcial|sem-lista|vazio|hub-antigo|so-dono` (padrão: completo). Nada de disco nem rede.

const DIA = 86_400_000
const agora = Date.UTC(2026, 10, 6, 17, 42)
const conversas = (bytes: number, count: number, diasAtras: number | null) => ({ bytes, count, oldestAt: diasAtras === null ? null : agora - diasAtras * DIA })

type Parte = number | null
const soma = (...p: Parte[]) => (p.some((x) => x === null) ? null : (p as number[]).reduce((a, b) => a + b, 0))

function bot(id: string, name: string, main: ReturnType<typeof conversas> | null, extras: ReturnType<typeof conversas> | null, anexos: Parte, memoria: [Parte, Parte], whatsapp: [Parte, Parte]) {
  const msgs = main && extras ? main.bytes + extras.bytes : null
  const mem = soma(...memoria), wa = soma(...whatsapp)
  return {
    id, name,
    messages: { main, extras, bytes: msgs },
    attachmentsBytes: anexos,
    memory: { rowsBytes: memoria[0], filesBytes: memoria[1], bytes: mem },
    whatsapp: { hubBytes: whatsapp[0], wacliBytes: whatsapp[1], bytes: wa },
    bytes: soma(msgs, anexos, mem, wa),
  }
}

const completos = () => [
  bot('donna', 'Donna', conversas(48_300_000, 12_840, 214), conversas(9_100_000, 1_932, 61), 312_400_000, [4_200_000, 860_000], [0, 0]),
  bot('prosa', 'Prosa', conversas(21_700_000, 5_410, 120), conversas(2_400_000, 388, 18), 96_800_000, [1_900_000, 240_000], [38_500_000, 512_000_000]),
  bot('sprint', 'Berga', conversas(6_200_000, 1_204, 45), conversas(0, 0, null), 4_100_000, [620_000, 0], [0, 0]),
  bot('crivo', 'Crivo', conversas(910_000, 96, 9), null, 0, [88_000, 12_000], [0, 0]),
]

export function relatorioDemo(): unknown {
  const cena = new URLSearchParams(location.search).get('armazenamento') ?? 'completo'
  if (cena === 'hub-antigo') throw { status: 404, message: 'unknown method' }
  if (cena === 'so-dono') throw { status: 403, message: 'owner_only' }
  const total = { databaseBytes: 1_840_000_000, walBytes: 12_600_000, databaseFreeBytes: 214_000_000, diskFreeBytes: 182_300_000_000 }
  if (cena === 'sem-lista') return { measuredAt: agora, total, bots: null }
  if (cena === 'vazio') return { measuredAt: agora, total: { databaseBytes: 2_400_000, walBytes: 0, databaseFreeBytes: 0, diskFreeBytes: 182_300_000_000 }, bots: [] }
  if (cena === 'parcial') {
    // WAL e espaço reaproveitável sem medida; Prosa sem anexos, arquivos de memória e pasta do WhatsApp.
    const [donna, , sprint] = completos()
    const prosa = bot('prosa', 'Prosa', conversas(21_700_000, 5_410, 120), conversas(2_400_000, 388, 18), null, [1_900_000, null], [38_500_000, null])
    return { measuredAt: agora, total: { ...total, walBytes: null, databaseFreeBytes: null }, bots: [donna, prosa, sprint] }
  }
  return { measuredAt: agora, total, bots: completos() }
}
