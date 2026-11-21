// Só para testes: quadros sintéticos sem câmera, sem Hub e sem dados reais.

/** JPEG sintético mínimo e válido na estrutura: APP0, SOF com o tamanho, SOS e fim. Não é decodificável, só legível pelo cabeçalho. */
export const jpegSintetico = (width: number, height: number, sof = 0xc0): Uint8Array =>
  Uint8Array.from([
    0xff, 0xd8,
    0xff, 0xe0, 0x00, 0x04, 0x4a, 0x46,
    0xff, sof, 0x00, 0x0b, 0x08, height >> 8, height & 0xff, width >> 8, width & 0xff, 0x01, 0x01, 0x11, 0x00,
    0xff, 0xda, 0x00, 0x02,
    0x01, 0x02,
    0xff, 0xd9,
  ])
