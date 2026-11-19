export type ShapeId =
  | 'oval' | 'pilula' | 'cubo' | 'gota' | 'ciclope' | 'tela' | 'urso'
  | 'cupula' | 'losango' | 'nuvem' | 'hexa' | 'gato' | 'totem';

export type EyeKey = 'pill' | 'round' | 'pixel' | 'bar' | 'cat' | 'cyc';
export type EyeId = 'auto' | EyeKey;

export type HeadId =
  | 'none' | 'cap' | 'helmet' | 'phones' | 'headset' | 'beret'
  | 'eyeshade' | 'bucket' | 'beanie' | 'flower' | 'pencil';
export type FaceId = 'none' | 'glasses' | 'shades';
export type EarsId = 'none' | 'hoops' | 'pearls';
export type ChinId = 'none' | 'beard' | 'stache';
export type NeckId = 'none' | 'bowtie' | 'tie' | 'scarf' | 'lanyard' | 'necklace';
export type CheeksId = 'none' | 'blush';
export type BadgeStyle = 'light' | 'dark' | 'color';

export type IconId =
  | 'sparkle' | 'clipboard' | 'briefcase' | 'kanban' | 'calendar' | 'notes'
  | 'search' | 'globe' | 'translate' | 'code' | 'bug' | 'bag'
  | 'box' | 'ship' | 'cart' | 'warehouse' | 'lifebuoy' | 'megaphone'
  | 'target' | 'chat' | 'pen' | 'seo' | 'video' | 'palette'
  | 'chart' | 'phone' | 'coin' | 'calc' | 'ledger' | 'people'
  | 'heart' | 'mail' | 'bolt' | 'rocket' | 'bulb' | 'mic';

/** Tudo que define a aparência de um bot. É isso que você salva no banco. */
export interface AvatarConfig {
  shape: ShapeId;
  /** Hex #RRGGBB. Qualquer cor funciona; a paleta é só sugestão. */
  color: string;
  eye: EyeId;
  head: HeadId;
  face: FaceId;
  ears: EarsId;
  chin: ChinId;
  neck: NeckId;
  cheeks: CheeksId;
  badge: IconId | 'none';
  badgeStyle: BadgeStyle;
}

/**
 * Estados visuais.
 * Contínuos (ficam até mudar): idle, listen, think, work, sleep.
 * Momentâneos (use useBotMood().play): blink, left, right, up, happy, surprise, wake.
 * drowsy é usado internamente pela sequência de acordar.
 */
export type Mood =
  | 'idle' | 'blink' | 'left' | 'right' | 'up' | 'listen' | 'think'
  | 'work' | 'happy' | 'surprise' | 'sleep' | 'drowsy' | 'wake';

export type BaseMood = 'idle' | 'listen' | 'think' | 'work' | 'sleep';
export type MomentAction = 'blink' | 'left' | 'right' | 'up' | 'happy' | 'surprise' | 'wake';

export interface RolePreset extends AvatarConfig {
  id: string;
  name: string;
  role: string;
  area: string;
  desc: string;
}

export type PartFill = 'dark' | 'tint' | 'gold' | 'body';

export interface PartDef {
  l: number; t: number; w: number; h: number;
  r?: string;
  fill: PartFill;
  tf?: string;
  or?: string;
  cp?: string;
}

export interface ShapeDef {
  label: string;
  /** [left, top, width, height, border-radius] em unidades de 1/10 do avatar */
  body: [number, number, number, number, string];
  btf?: string;
  bcp?: string;
  bsh?: string;
  visor: [number, number, number, number, string];
  eye: EyeKey;
  gap: number;
  look: number;
  parts: PartDef[];
  deco?: PartDef[];
  /** [centroX, linhaDaAba, largura] onde chapéus apoiam */
  hat: [number, number, number];
  /** [y, xEsquerda, xDireita] onde ficam fones e brincos */
  side: [number, number, number];
  /** base do corpo (y) */
  by: number;
  /** largura do pescoço para cachecol/colar */
  nw: number;
  /** posição do selo [left, top] */
  badge: [number, number];
}

/** Uma peça desenhada (div absoluta), já com unidades. */
export interface FacePart {
  l: string; t: string; w: string; h: string; r: string;
  bg: string; bw: string; bc: string;
  tf: string; or: string; cp: string;
  op: number; sh: string;
}

export interface FaceEye {
  w: string; h: string; r: string; tf: string; bg: string; sh: string;
}

export interface LifeState {
  blink: boolean;
  glancing: boolean;
  gx: number;
  gy: number;
}

export interface FaceModel {
  fs: string;
  bob: string;
  pose: string;
  filter: string;
  color: string;
  body: { l: string; t: string; w: string; h: string; r: string; tf: string; cp: string; sh: string };
  visor: { l: string; t: string; w: string; h: string; r: string };
  gap: string;
  look: string;
  eyes: FaceEye[];
  back: FacePart[];
  front: FacePart[];
  scan: boolean;
  scanColor: string;
  zzz: boolean;
  dots: boolean;
  waves: boolean;
  tint: string;
  badge: null | { l: string; t: string; bg: string; fg: string; ring: string; paths: string[] };
}
