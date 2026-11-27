// A marca de cada serviço do catálogo, em arquivo local do app (nada vem da rede em tempo de execução). A chave é a
// `marca` que o Hub manda em cada serviço (host/src/fork/conectores/catalogo_fichas.rs): o nome do arquivo, sem extensão. Origem, data e uso de cada arquivo:
// THIRD_PARTY.md (seção "Marcas de serviços") e TRADEMARK.md. Serviço sem arquivo aqui fica com o globo.
import airtable from '../../assets/marcas/airtable.png'
import apify from '../../assets/marcas/apify.ico'
import atlassian from '../../assets/marcas/atlassian.svg'
import attio from '../../assets/marcas/attio.svg'
import betterStack from '../../assets/marcas/better-stack.svg'
import brevo from '../../assets/marcas/brevo.svg'
import browserUse from '../../assets/marcas/browser-use.svg'
import calCom from '../../assets/marcas/cal-com.svg'
import calendly from '../../assets/marcas/calendly.svg'
import camtasia from '../../assets/marcas/camtasia.svg'
import clerk from '../../assets/marcas/clerk.svg'
import clickup from '../../assets/marcas/clickup.svg'
import cloudflare from '../../assets/marcas/cloudflare.svg'
import coda from '../../assets/marcas/coda.png'
import coderabbit from '../../assets/marcas/coderabbit.svg'
import composio from '../../assets/marcas/composio.png'
import context7 from '../../assets/marcas/context7.svg'
import descript from '../../assets/marcas/descript.svg'
import donoA from '../../assets/marcas/dono-a.svg'
import exa from '../../assets/marcas/exa.svg'
import expo from '../../assets/marcas/expo.png'
import fal from '../../assets/marcas/fal.png'
import firecrawl from '../../assets/marcas/firecrawl.svg'
import github from '../../assets/marcas/github.png'
import gmail from '../../assets/marcas/gmail.png'
import googleAgenda from '../../assets/marcas/google-agenda.png'
import googleDrive from '../../assets/marcas/google-drive.png'
import granola from '../../assets/marcas/granola.svg'
import heygen from '../../assets/marcas/heygen.ico'
import higgsfield from '../../assets/marcas/higgsfield.png'
import instagram from '../../assets/marcas/instagram.webp'
import jungleScout from '../../assets/marcas/jungle-scout.svg'
import linear from '../../assets/marcas/linear.svg'
import lovable from '../../assets/marcas/lovable.png'
import mem0 from '../../assets/marcas/mem0.svg'
import metricool from '../../assets/marcas/metricool.png'
import monday from '../../assets/marcas/monday.svg'
import neon from '../../assets/marcas/neon.svg'
import notion from '../../assets/marcas/notion.png'
import openrouter from '../../assets/marcas/openrouter.png'
import outlook from '../../assets/marcas/outlook.png'
import paypal from '../../assets/marcas/paypal.svg'
import pipedrive from '../../assets/marcas/pipedrive.png'
import plaud from '../../assets/marcas/plaud.svg'
import posthog from '../../assets/marcas/posthog.svg'
import railway from '../../assets/marcas/railway.png'
import render from '../../assets/marcas/render.svg'
import resend from '../../assets/marcas/resend.png'
import revenuecat from '../../assets/marcas/revenuecat.svg'
import semrush from '../../assets/marcas/semrush.svg'
import sentry from '../../assets/marcas/sentry.svg'
import shortIo from '../../assets/marcas/short-io.png'
import slack from '../../assets/marcas/slack.png'
import stripe from '../../assets/marcas/stripe.svg'
import supabase from '../../assets/marcas/supabase.svg'
import tally from '../../assets/marcas/tally.svg'
import tavily from '../../assets/marcas/tavily.svg'
import tiktokAds from '../../assets/marcas/tiktok-ads.png'
import tinyfish from '../../assets/marcas/tinyfish.svg'
import todoist from '../../assets/marcas/todoist.svg'
import trello from '../../assets/marcas/trello.ico'
import vidiq from '../../assets/marcas/vidiq.png'
import wisprFlow from '../../assets/marcas/wispr-flow.png'
import wix from '../../assets/marcas/wix.png'

export const MARCAS: Readonly<Record<string, string>> = {
  airtable,
  apify,
  atlassian,
  attio,
  'better-stack': betterStack,
  brevo,
  'browser-use': browserUse,
  'cal-com': calCom,
  calendly,
  camtasia,
  clerk,
  clickup,
  cloudflare,
  coda,
  coderabbit,
  composio,
  context7,
  descript,
  'dono-a': donoA,
  exa,
  expo,
  fal,
  firecrawl,
  github,
  gmail,
  'google-agenda': googleAgenda,
  'google-drive': googleDrive,
  granola,
  heygen,
  higgsfield,
  instagram,
  'jungle-scout': jungleScout,
  linear,
  lovable,
  mem0,
  metricool,
  monday,
  neon,
  notion,
  openrouter,
  outlook,
  paypal,
  pipedrive,
  plaud,
  posthog,
  railway,
  render,
  resend,
  revenuecat,
  semrush,
  sentry,
  'short-io': shortIo,
  slack,
  stripe,
  supabase,
  tally,
  tavily,
  'tiktok-ads': tiktokAds,
  tinyfish,
  todoist,
  trello,
  vidiq,
  'wispr-flow': wisprFlow,
  wix,
}

/** O arquivo da `marca` dada, ou `null` (o chamador usa o globo). Só chave conhecida; nunca um caminho vindo de fora. */
export const arquivoDaMarca = (marca: string | null | undefined): string | null =>
  marca && Object.hasOwn(MARCAS, marca) ? MARCAS[marca] : null
