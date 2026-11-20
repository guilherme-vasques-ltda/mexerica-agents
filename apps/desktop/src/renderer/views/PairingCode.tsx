// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useCallback, useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import type { PairingInfo } from '@shared/models'
import { IconButton, Spinner } from '../components/Controls'
import { HStack, VStack } from '../components/Stack'
import { Text } from '../components/Text'
import { useScreenViewed } from '../store/analytics'
import './pairing-code.css'
import { formatarData } from '../lib/fork-i18n/index.ts'
import { useIdioma, useT } from '../lib/fork-i18n/estado'
import { comNos, no } from './settings/rotulo'

/**
 * A one-time pairing QR (v3) from a computer this app manages: its keys, a code, and how to reach it.
 * A new code comes when `renew` changes and when this one expires on screen; when none comes, the
 * error takes the place of the old code, which may have expired.
 */
/** O `name` do erro de `requestCode` sem o host deste computador: o texto sai do catálogo ao desenhar (R1 do #575). */
const SEM_HOST = 'PairingWithoutHost'

export function PairingCode({ name, cloudOn, requestCode, renew = 0 }: { name: string; cloudOn: boolean; requestCode: () => Promise<PairingInfo>; renew?: number }) {
  const t = useT()
  const idioma = useIdioma()
  const [code, setCode] = useState<{ info: PairingInfo; qr: string } | null>(null)
  // O erro do Hub chega pronto; o nosso fica como marca e é traduzido ao desenhar.
  const [error, setError] = useState<{ semHost: true } | { texto: string } | null>(null)
  useScreenViewed('pairing', renew)

  // Loads overlap (expiry, "New code", a renew): only the latest one's code or error shows.
  const latest = useRef(0)
  const load = useCallback(async () => {
    const request = ++latest.current
    try {
      const info = await requestCode()
      const qr = await QRCode.toDataURL(info.pairingUrl, { errorCorrectionLevel: 'M', margin: 0, width: 400, color: { dark: '#000000', light: '#ffffff' } })
      if (request !== latest.current) return
      setCode({ info, qr })
      setError(null)
    } catch (e) {
      if (request !== latest.current) return
      setCode(null)
      setError(e instanceof Error && e.name === SEM_HOST ? { semHost: true } : { texto: e instanceof Error ? e.message : String(e) })
    }
  }, [requestCode])

  useEffect(() => void load(), [load, renew])
  const expiresAt = code?.info.expiresAt
  useEffect(() => {
    if (!expiresAt) return
    const timer = setTimeout(() => void load(), Math.max(1000, expiresAt - Date.now()))
    return () => clearTimeout(timer)
  }, [expiresAt, load])

  return (
    <VStack align="center" gap={12} className="pairing">
      {code ? (
        <>
          <div className="pairing-qr fade-in">
            <img src={code.qr} alt={t('navegacao.parear.qr', { computador: name })} width={200} height={200} />
          </div>
          <Text as="div" size="caption" tone="secondary" className="pairing-note">
            {t('marca.parear.iphone')}
          </Text>
          {!cloudOn ? (
            <Text as="div" size="caption" tone="attention" className="pairing-note">
              {code.info.urls.length === 0
                ? t('navegacao.parear.semRede')
                : comNos(t('navegacao.parear.mesmaRede', { computador: no('computador') }), { computador: <bdi dir="auto">{name}</bdi> })}
            </Text>
          ) : null}
          <HStack gap={4}>
            {expiresAt ? (
              <Text size="caption2" tone="tertiary">
                {t('navegacao.parear.valeAte', { hora: formatarData(idioma, expiresAt, { hour: 'numeric', minute: '2-digit' }) })}
              </Text>
            ) : null}
            <IconButton title={t('navegacao.parear.novo')} icon="arrow.clockwise" onClick={() => void load()} />
            <IconButton title={t('navegacao.parear.copiar')} icon="doc.on.doc" onClick={() => window.codync.app.copy(code.info.pairingUrl)} />
          </HStack>
        </>
      ) : error ? (
        <>
          <Text as="div" size="caption" tone="danger" className="pairing-note fade-in" dir={'texto' in error ? 'auto' : undefined}>{'texto' in error ? error.texto : t('navegacao.parear.semHost')}</Text>
          <IconButton title={t('navegacao.janela.tentarDeNovo')} icon="arrow.clockwise" onClick={() => void load()} />
        </>
      ) : (
        <Spinner size={20} />
      )}
    </VStack>
  )
}
