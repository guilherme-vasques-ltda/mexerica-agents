// Núcleo (sem React)
export * from './core/types';
export { SHAPES, EYE_TYPES, ICONS, NONE_ICON, PALETTE, LABELS, ROLES, DEFAULT_CONFIG, configFromRole } from './core/data';
export { buildFace, hexToRgb, tintOf, STILL_LIFE } from './core/model';

// React
export { BotFace } from './react/BotFace';
export type { BotFaceProps } from './react/BotFace';
export { AvatarEditor } from './react/AvatarEditor';
export type { AvatarEditorProps, EditorTab } from './react/AvatarEditor';
export { useBotLife, useBotMood } from './react/hooks';
export { ensureBotFaceStyles, BOTFACE_CSS } from './react/styles';
