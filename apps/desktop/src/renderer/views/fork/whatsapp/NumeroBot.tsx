import { useContext, useEffect, useState } from 'react'
import { Icon } from '../../../components/Icon'
import { formatarTelefone } from './caixa'
import { finalDoTelefone, mascararTelefone, numeroAMostra, numeroAtivo, revelar, vigiarRevelado, type Revelacao } from './numero-bot'
import { useT } from '../../../lib/fork-i18n/estado'
import { SuperficieAVista } from './superficie'

// D374: o número conectado do bot, sempre por aqui (aba do painel, tela grande, avisos). Escondido por padrão.
// `ativo`: a superfície está à vista. O painel e a tela grande continuam montados escondidos por CSS; fora de vista o
// número revelado nem é desenhado (nada no DOM oculto) e a revelação acaba: ao voltar, aparece escondido.

// D387: o mesmo componente para contatos e grupos sem nome salvo e para a conta de saída nos cartões. Sem `ativo`, vale
// a superfície em volta (SuperficieAVista). `olho={false}`: só a máscara, onde o componente fica dentro de outro botão
// (a linha da lista); o olhinho fica no cabeçalho da conversa.

export function NumeroBot({ phone, ativo: dado, olho = true }: { phone: string; ativo?: boolean; olho?: boolean }) {
  const t = useT()
  const aVista = useContext(SuperficieAVista)
  const ativo = numeroAtivo(dado, aVista, olho)
  const [r, setR] = useState<Revelacao>({ pedido: false })
  // Saiu de vista: esquece a revelação já no render (sem quadro com o número no DOM escondido).
  if (!ativo && r.pedido) setR(revelar(r, { tipo: 'superficie', ativo }))
  const visivel = numeroAMostra(r, ativo)
  const acao = t(visivel ? 'whatsapp.numero.esconder' : 'whatsapp.numero.mostrar')
  useEffect(() => (visivel ? vigiarRevelado(() => setR((x) => revelar(x, { tipo: 'esconder' })), window, document) : undefined), [visivel])
  return (
    <span className="wa-num">
      {visivel
        ? <span className="wa-num-t"><bdi dir="ltr">{formatarTelefone(phone)}</bdi></span>
        : <span className="wa-num-t escondido" role="img" aria-label={t('whatsapp.numero.escondido', { fim: finalDoTelefone(phone) })}><bdi dir="ltr">{mascararTelefone(phone)}</bdi></span>}
      {olho ? <button type="button" className="wa-num-olho" aria-pressed={visivel} aria-label={acao}
        title={acao} onClick={() => setR((x) => revelar(x, { tipo: 'olhinho' }))}>
        <Icon name={visivel ? 'eye.slash' : 'eye'} size={12} scaled={false} />
      </button> : null}
    </span>
  )
}
