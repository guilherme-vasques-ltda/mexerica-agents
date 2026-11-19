import { useEffect, useState } from 'react'
import type { Entry } from '@shared/models'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { formatarData } from '../../../lib/fork-i18n/index.ts'
import { cru, frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { errorText, market, loginTitle, type ConnectionRequest, type SavedLogin } from '../../marketplace/market-models'
import { ServiceLogo } from '../../marketplace/pieces'
import { extensoesScreen } from '../extensoes/tela'
import { faseDo, nomeGuardado } from './estado'
import './cofre-seguro.css'

/**
 * MX-209: o "pedido seguro" de login ou chave na conversa. Desenhado pelo app, sem campo de valor: a senha é
 * digitada na janela própria (cartão seguro) e nunca passa por aqui. O estado "guardado" chega pela entrada do Hub.
 */
export function PedidoSeguro({ entry }: { entry: Entry }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const request = entry.data.connectionRequest as unknown as ConnectionRequest
  const fase = faseDo(request)
  const login = request.kind === 'login'
  const codigo = request.kind === 'code'
  const botNome = store.bots.get(entry.botId)?.name ?? ''
  const [aberta, setAberta] = useState(false)
  const [adiado, setAdiado] = useState(false)
  const [busy, setBusy] = useState(false)
  const [abrindo, setAbrindo] = useState(false)
  const [erro, setErro] = useState<Frase | null>(null)
  // D82: logins já salvos para o site; escolher um liga ao bot que pediu (só o id vai ao Hub, nunca a senha).
  const [salvos, setSalvos] = useState<SavedLogin[]>([])
  const online = store.connection.kind === 'online'

  useEffect(() => window.codync.forkCofre.onEstado((aviso) => {
    if (aviso.entryId !== entry.id) return
    setAberta(aviso.estado === 'aberta')
    if (aviso.estado !== 'aberta') setErro(null)
  }), [entry.id])

  useEffect(() => {
    const client = store.client
    if (!login || fase !== 'pendente' || !client) return
    let live = true
    market.logins(client).then((all) => live && setSalvos(all.filter((l) => l.site === request.site)), () => {})
    return () => {
      live = false
    }
  }, [login, fase, request.site, store, online])

  const usar = async (salvo: SavedLogin) => {
    const client = store.client
    if (!client) return
    setBusy(true)
    setErro(null)
    try {
      await market.finishConnectionRequest(client, entry.id, { loginId: salvo.id })
    } catch (e) {
      setErro(cru(errorText(e)))
    }
    setBusy(false)
  }

  const digitar = async () => {
    // Relay (nuvem) não entra: o valor só vai por um caminho que o app controla de ponta a ponta.
    // O main também coalesce aberturas do mesmo pedido; aqui o botão só não dispara de novo.
    if (abrindo || !store.isLoopback) return abrindo ? undefined : setErro(frase('extensoes.cofre.erro.semSuporte'))
    setErro(null)
    setAbrindo(true)
    try {
      const client = store.client
      // Só a referência do pedido: o main busca a entrada no Hub e deriva todos os textos da janela.
      const resposta = await window.codync.forkCofre.abrir({
        computerId: store.computer.id, entryId: entry.id,
        ...(client?.spaceId ? { spaceId: client.spaceId } : {}),
        ...(entry.spaceId ? { spaceId: entry.spaceId } : {}),
        ...(client?.boundHubId ? { hubId: client.boundHubId } : {}),
        ...(entry.conversationId ? { conversationId: entry.conversationId } : {}),
      })
      if (!resposta.ok) setErro(frase(resposta.motivo === 'semSuporte' ? 'extensoes.cofre.erro.semSuporte' : 'extensoes.cofre.erro.abrir'))
    } catch {
      setErro(frase('extensoes.cofre.erro.abrir'))
    } finally {
      setAbrindo(false)
    }
  }

  const verNoCofre = () => extensoesScreen.show('credentials', store.computer.id)
  // Login e código mostram o site; o código também mostra o título do login (vindo do Hub, nunca do bot).
  const alvo = login ? (request.site ?? request.title) : request.title
  const siteDoCodigo = codigo && request.site && request.site !== request.title ? request.site : null

  if (fase === 'cancelado') {
    return (
      <HStack gap={10} className="pedido-seguro idioma-superficie" {...superficieIdioma(idioma)}>
        <span className="pedido-seguro-selo cancelado">{t('extensoes.pedido.cancelado')}</span>
        <Text size="callout" tone="secondary" truncate dir="auto">{t(codigo ? 'extensoes.cofre.tituloCodigo' : login ? 'extensoes.cofre.tituloSenha' : 'extensoes.cofre.tituloChave', { bot: botNome })}</Text>
      </HStack>
    )
  }

  // "Código entregue": o código não é guardado (vive só na memória do Hub, uma vez, por 5 minutos); não há "Ver no cofre".
  if (fase === 'guardado' && codigo) {
    return (
      <VStack gap={12} className="pedido-seguro idioma-superficie" {...superficieIdioma(idioma)}>
        <HStack gap={10}>
          <span className="pedido-seguro-selo guardado"><Icon name="checkmark" size={11} weight="semibold" /> {t('extensoes.cofre.codigoEntregue')}</span>
          <Text size="caption" tone="secondary" className="cofre-origem">{t('extensoes.cofre.origem')}</Text>
        </HStack>
        <PedidoLinhas t={t} alvo={alvo} campo={siteDoCodigo} botNome={botNome} codigo />
        <HStack gap={6} className="pedido-seguro-rodape">
          <Icon name="checkmark.shield" size={12} className="icone-ok" />
          <span>{t('extensoes.cofre.codigoEntregueTexto')}</span>
        </HStack>
      </VStack>
    )
  }

  if (fase === 'guardado') {
    const n = nomeGuardado(request)
    const hora = formatarData(idioma, entry.updatedAt, { hour: '2-digit', minute: '2-digit' })
    return (
      <VStack gap={12} className="pedido-seguro idioma-superficie" {...superficieIdioma(idioma)}>
        <HStack gap={10}>
          <span className="pedido-seguro-selo guardado"><Icon name="checkmark" size={11} weight="semibold" /> {t('extensoes.cofre.guardado')}</span>
          <Text size="caption" tone="secondary" className="cofre-origem">{t('extensoes.cofre.origem')}</Text>
        </HStack>
        <PedidoLinhas t={t} alvo={alvo} botNome={botNome} />
        <HStack gap={10}>
          <HStack as="span" gap={6} className="pedido-seguro-nome">
            <Icon name="key" size={12} />
            <Text size="callout" weight="semibold" dir="auto">
              {n.login ? t('extensoes.cofre.nomeSenha', { site: n.site }) : t('extensoes.cofre.nomeChave', { campo: n.campo, conector: n.conector })}
            </Text>
          </HStack>
          {/* Lugar que já existe: Extensões → Credenciais (logins e chaves). */}
          <button className="link-cofre" onClick={verNoCofre}>{t('extensoes.cofre.verNoCofre')}</button>
        </HStack>
        <HStack gap={6} className="pedido-seguro-rodape">
          <Icon name="checkmark.shield" size={12} className="icone-ok" />
          <span>{t(login ? 'extensoes.cofre.guardadaSenha' : 'extensoes.cofre.guardadaChave', { hora, bot: botNome })}</span>
        </HStack>
      </VStack>
    )
  }

  return (
    <VStack gap={12} className="pedido-seguro idioma-superficie" {...superficieIdioma(idioma)}>
      <HStack gap={10}>
        <span className="pedido-seguro-selo"><Icon name="clock" size={11} weight="semibold" /> {t('extensoes.cofre.selo')}</span>
        <Text size="caption" tone="secondary">{t('extensoes.cofre.origem')}</Text>
      </HStack>
      {adiado ? (
        <HStack gap={10}>
          <VStack grow><Text size="callout" tone="secondary" dir="auto">{t(login ? 'extensoes.cofre.adiadoSenha' : 'extensoes.cofre.adiadoChave')}</Text></VStack>
          <Button kind="secondary" onClick={() => setAdiado(false)}>{t('extensoes.cofre.retomar')}</Button>
        </HStack>
      ) : (
        <>
          <HStack gap={12}>
            <ServiceLogo site={(login || codigo) && request.site?.includes('.') ? request.site : null} name={request.title} registryName={request.registryName} />
            <VStack gap={2} grow>
              <Text size="body" weight="semibold" tone="text" dir="auto">{t(codigo ? 'extensoes.cofre.tituloCodigo' : login ? 'extensoes.cofre.tituloSenha' : 'extensoes.cofre.tituloChave', { bot: botNome })}</Text>
              {entry.data.text ? <Text size="subheadline" tone="secondary" lines={3} title={entry.data.text} dir="auto">{entry.data.text}</Text> : null}
            </VStack>
          </HStack>
          <PedidoLinhas t={t} alvo={alvo} campo={codigo ? siteDoCodigo : login ? null : request.field ?? null} botNome={botNome} codigo={codigo} />
          {salvos.length ? (
            <>
              <Text size="caption" tone="secondary">{t('extensoes.pedido.salvosPara', { site: `⁦${request.site ?? request.title}⁩` })}</Text>
              {salvos.map((l) => (
                <HStack key={l.id} gap={8}>
                  <VStack grow><Text size="subheadline" tone="text" truncate dir="auto">{loginTitle(l)}</Text></VStack>
                  <Button kind="secondary" disabled={busy} onClick={() => void usar(l)}>{t('extensoes.pedido.usar')}</Button>
                </HStack>
              ))}
            </>
          ) : null}
          {aberta ? (
            <Text size="callout" tone="secondary">{t('extensoes.cofre.janelaAberta')}</Text>
          ) : null}
          <HStack gap={8} wrap>
            <Button disabled={abrindo} onClick={() => void digitar()}>
              <Icon name="lock" size={12} weight="semibold" /> {t(aberta ? 'extensoes.cofre.irParaJanela' : codigo ? 'extensoes.cofre.digitarCodigo' : login ? 'extensoes.cofre.digitarSenha' : 'extensoes.cofre.digitarChave')}
            </Button>
            <Button kind="secondary" onClick={() => setAdiado(true)}>{t('extensoes.cofre.agoraNao')}</Button>
          </HStack>
          <HStack gap={6} className="pedido-seguro-rodape">
            <Icon name="shield" size={12} className="icone-ok" />
            <span>{t(codigo ? 'extensoes.cofre.rodapeCodigo' : login ? 'extensoes.cofre.rodapeSenha' : 'extensoes.cofre.rodapeChave')}</span>
          </HStack>
        </>
      )}
      {erro ? <Text size="footnote" tone="danger">{textoDe(erro, idioma)}</Text> : null}
    </VStack>
  )
}

function PedidoLinhas({ t, alvo, campo, botNome, codigo = false }: { t: ReturnType<typeof useT>; alvo: string; campo?: string | null; botNome: string; codigo?: boolean }) {
  return (
    <dl className="pedido-seguro-linhas">
      <dt>{t('extensoes.cofre.para')}</dt>
      <dd><bdi dir="ltr" className="cofre-ltr">{alvo}</bdi>{campo ? <> · <bdi dir="ltr" className="cofre-ltr">{campo}</bdi></> : null}</dd>
      {/* O código não fica guardado: a linha diz a validade em vez de "Fica guardada". */}
      <dt>{t(codigo ? 'extensoes.cofre.validade' : 'extensoes.cofre.fica')}</dt>
      <dd>{codigo ? t('extensoes.cofre.codigoVale') : t('extensoes.cofre.ficaValor', { bot: botNome })}</dd>
    </dl>
  )
}
