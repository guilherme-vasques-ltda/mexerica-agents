// Telas v3 02: "Adicionar um conector seu" (um MCP do próprio usuário, por link ou comando). Fora do catálogo, marcado
// "seu, não verificado", com a confirmação extra quando é um comando (roda um programa no computador, com as permissões
// da pessoa). O Hub confere nome, Space e endereço; a chave vai direto ao cofre e nunca volta nem passa pelo bot.
import { useState } from 'react'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import type { PedidoPersonalizado } from './api.ts'
import { pedidoDoPersonalizado, type ModoPersonalizado as Modo } from './regras.ts'
import { Aviso, Erro, frasesDaRecusa } from './pecas'

export function Personalizado({ adicionar, aoCancelar, aoAdicionado }: {
  adicionar: (pedido: PedidoPersonalizado) => Promise<void>
  aoCancelar: () => void
  aoAdicionado: (nome: string) => void
}) {
  const t = useT()
  const [modo, setModo] = useState<Modo>('link')
  const [alvo, setAlvo] = useState('')
  const [nome, setNome] = useState('')
  // A chave mora só aqui, até o envio ao Hub; some com a tela.
  const [chave, setChave] = useState('')
  const [entendi, setEntendi] = useState(false)
  const [entendiComando, setEntendiComando] = useState(false)
  const [ocupado, setOcupado] = useState(false)
  const [erro, setErro] = useState<unknown>(null)
  const pronto = alvo.trim() !== '' && nome.trim() !== '' && entendi && (modo === 'link' || entendiComando) && !ocupado
  const enviar = async () => {
    if (!pronto) return
    setOcupado(true); setErro(null)
    try { await adicionar(pedidoDoPersonalizado(modo, alvo, nome, chave)); setChave(''); aoAdicionado(nome.trim()) } catch (e) { setErro(e); setOcupado(false) }
  }
  return (
    <div className="mercado">
      <div className="mercado-topo">
        <button className="mercado-voltar" onClick={aoCancelar}><Icon name="arrow.left" size={12} /><Text size="callout">{t('personalizado.voltar')}</Text></button>
        <HStack gap={6} className="mercado-etiqueta aviso"><Icon name="shield" size={11} /><Text size="caption" weight="semibold">{t('personalizado.selo')}</Text></HStack>
      </div>
      <div className="mercado-conectar">
        <div className="mercado-ficha-principal">
          <div className="mercado-cabeca">
            <span className="mercado-tile"><Icon name="puzzlepiece.extension" size={22} /></span>
            <VStack gap={2} shrink>
              <Text as="h2" size="title2" weight="semibold" tone="text">{t('personalizado.titulo')}</Text>
              <Text size="callout" tone="secondary">{t('personalizado.sub')}</Text>
            </VStack>
          </div>
          <section className="mercado-secao">
            <Text as="h3" size="headline" weight="semibold" tone="text">{t('personalizado.oque')}</Text>
            <HStack gap={12}>
              <div className="mercado-segmentos" role="radiogroup" aria-label={t('personalizado.modo.rotulo')}>
                {(['link', 'comando'] as const).map((m) => (
                  <button key={m} role="radio" aria-checked={modo === m} className="mercado-segmento" onClick={() => { setModo(m); setErro(null) }}>
                    <Text size="callout" weight="medium">{t(m === 'link' ? 'personalizado.modo.link' : 'personalizado.modo.comando')}</Text>
                  </button>
                ))}
              </div>
              {modo === 'comando' ? <Text size="callout" tone="secondary">{t('personalizado.modo.dicaComando')}</Text> : null}
            </HStack>
          </section>
          <label className="mercado-cartao-campo">
            <Text size="callout" weight="semibold" tone="text">{t(modo === 'link' ? 'personalizado.link.titulo' : 'personalizado.comando.titulo')}</Text>
            <input className="mercado-campo mercado-mono" autoComplete="off" spellCheck={false} autoCapitalize="off" value={alvo} aria-invalid={erro !== null}
              placeholder={t(modo === 'link' ? 'personalizado.link.exemplo' : 'personalizado.comando.exemplo')} onChange={(e) => { setAlvo(e.target.value); setErro(null) }} />
            <Text size="callout" tone="secondary">{t(modo === 'link' ? 'personalizado.link.dica' : 'personalizado.comando.dica')}</Text>
          </label>
          <label className="mercado-cartao-campo">
            <Text size="callout" weight="semibold" tone="text">{t('personalizado.nome.titulo')}</Text>
            <input className="mercado-campo" autoComplete="off" value={nome} aria-invalid={erro !== null} onChange={(e) => { setNome(e.target.value); setErro(null) }} />
            <Text size="callout" tone="secondary">{t('personalizado.nome.dica')}</Text>
          </label>
          {modo === 'link' ? (
            <div className="mercado-bloco">
              <HStack gap={8}>
                <Icon name="lock" size={13} />
                <Text size="body" weight="semibold" tone="text">{t('personalizado.chave.titulo')}</Text>
                <Text size="caption" weight="medium" className="mercado-etiqueta neutra">{t('google.cartao.selo')}</Text>
              </HStack>
              <Text size="callout" tone="secondary">{t('personalizado.chave.texto')}</Text>
              <input className="mercado-campo mercado-mono" type="password" autoComplete="off" spellCheck={false} value={chave} placeholder={t('personalizado.chave.placeholder')} aria-label={t('personalizado.chave.titulo')} onChange={(e) => setChave(e.target.value)} />
            </div>
          ) : null}
          {erro !== null ? <Erro>{frasesDaRecusa(t, erro, nome)}</Erro> : null}
        </div>
        <aside className="mercado-ficha-lateral">
          <div className="mercado-bloco mercado-nao-verificado">
            <HStack gap={10} align="baseline"><Icon name="shield" size={14} /><Text size="headline" weight="semibold" tone="text">{t('personalizado.aviso.titulo')}</Text></HStack>
            <Text size="callout" tone="secondary">{t('personalizado.aviso.texto')}</Text>
          </div>
          <Text size="callout" weight="semibold" tone="secondary">{t('personalizado.entra.titulo')}</Text>
          <div className="mercado-lista-fina">
            <VStack gap={2}><Text size="body" weight="semibold" tone="text">{t('personalizado.entra.desligado.titulo')}</Text><Text size="callout" tone="secondary">{t('personalizado.entra.desligado.texto')}</Text></VStack>
            <VStack gap={2}><Text size="body" weight="semibold" tone="text">{t('personalizado.entra.sua.titulo')}</Text><Text size="callout" tone="secondary">{t('personalizado.entra.sua.texto')}</Text></VStack>
          </div>
        </aside>
      </div>
      <div className="mercado-rodape">
        <VStack gap={6} shrink>
          <label className="mercado-opcao"><input type="checkbox" checked={entendi} onChange={(e) => setEntendi(e.target.checked)} /><Text size="body" tone="text">{t('personalizado.entendi')}</Text></label>
          {modo === 'comando' ? <label className="mercado-opcao"><input type="checkbox" checked={entendiComando} onChange={(e) => setEntendiComando(e.target.checked)} /><Text size="body" tone="text">{t('personalizado.entendiComando')}</Text></label> : null}
        </VStack>
        <div className="mercado-acoes">
          <button className="mercado-botao-texto" onClick={aoCancelar}><Text size="body" weight="medium">{t('personalizado.cancelar')}</Text></button>
          <Button onClick={() => void enviar()} disabled={!pronto}>{t(ocupado ? 'personalizado.adicionando' : 'personalizado.adicionar')}</Button>
        </div>
      </div>
      {ocupado ? <Aviso>{t('personalizado.entrando')}</Aviso> : null}
    </div>
  )
}
